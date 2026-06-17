use std::io::Write;

use tracing::debug;
use tracing::error;

use crate::entities::backend::EntryKind;
use crate::entities::common;
use crate::entities::common::under_prefix;
use crate::entities::file_index;
use crate::entities::file_index::DuplicateGroup;
use crate::entities::file_info;
use crate::entities::uri::Location;

use super::copy::Source;
use super::report::FindReport;

const FEATURE_FIND: &str = "find";

// 返回完整 FindReport（scanned = 入索引文件数；bytes_read 来自 Index 累计；
// groups 为 DuplicateGroup 列表），dispatch 层直接落 JSON 而无需重新统计。
//
// # Errors
//
// output 路径不存在或不是目录时返回 `Err`：空报告 + exit 0 与"无重复"不可区分，
// 会误导基于退出码/空脚本做删除决策的调用方。
pub(crate) fn find_duplicates(
    secure: bool,
    sources: Vec<Source>,
    output: Option<&Source>,
) -> common::Result<FindReport> {
    let mut index = file_index::Index::new();

    if let Some((loc, backend)) = output {
        let loc_str = loc.display();
        // NotFound 与 Ok(非目录) 对用户语义等同——「output 路径不是可用目录」；其它
        // ErrorKind（PermissionDenied / 网络 / 远端协议异常）必须传播原 Err，曾用
        // `is_ok_and` 把它们一起吞成 "not a directory" 致排查方向被误导。
        let missing_or_non_dir = match backend.metadata(loc) {
            Ok(m) => m.kind != EntryKind::Dir,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => {
                error!(
                    feature = FEATURE_FIND,
                    operation = "validate_output",
                    result = "metadata_error",
                    output = %loc_str,
                    error = %e,
                    "cannot access output path"
                );
                return Err(common::Error::Io(e));
            }
        };
        if missing_or_non_dir {
            error!(
                feature = FEATURE_FIND,
                operation = "validate_output",
                result = "not_a_directory",
                output = %loc_str,
                "output path is not a directory"
            );
            return Err(common::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("output path is not a directory: {loc_str}"),
            )));
        }
    }

    for (loc, backend) in sources {
        index.visit_location(&loc, &backend);
    }

    let scan_stats = index.stats();
    let scanned = index.files().len();
    let bytes_read = index.bytes_read();
    debug!(
        feature = FEATURE_FIND,
        operation = "scan_complete",
        result = "ok",
        secure,
        files = scanned,
        similar_files = index.similar_files().len(),
        bytes_read,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "index built"
    );

    let groups = if secure {
        index.search_same()
    } else {
        index.fast_search_same()
    };
    debug!(
        feature = FEATURE_FIND,
        operation = "search_same",
        result = "ok",
        secure,
        groups = groups.len(),
        "duplicate groups discovered"
    );

    let prefix_owned = compute_output_prefix(output);

    render_script(&groups, prefix_owned.as_deref(), &mut std::io::stdout());

    debug!(
        feature = FEATURE_FIND,
        operation = "finalize",
        result = "ok",
        bytes_read,
        "find_duplicates done"
    );

    Ok(FindReport {
        scanned,
        bytes_read,
        groups: groups
            .into_iter()
            .map(|g| g.paths.into_iter().map(|p| p.to_string()).collect())
            .collect(),
    })
}

// 上方 is_dir 断言已过滤非目录；canonicalize 仍可能因 TOCTOU（验证后被外部
// 删除）失败。与 copy/run.rs::canonical_prefix 同口径回退原路径，避免 expect
// 触发进程崩溃（并发 cleanup / 自动备份脚本下可复现）。
#[doc(hidden)]
#[must_use]
pub fn compute_output_prefix(output: Option<&Source>) -> Option<String> {
    output.map(|(loc, _)| match loc {
        // 显式 match 替代 `.map_or_else(closure, closure)`：multi-binary instance 下
        // closure 在每个 binary 中独立编译，未被该 binary 触发即算 region miss；
        // match arm 让 LLVM 把同名 region 在 instance 间合并。
        Location::Local(p) => match file_info::full_path(p.as_str()) {
            Ok(fp) => fp.as_str().to_string(),
            Err(_) => p.as_str().to_string(),
        },
        other => other.display(),
    })
}

/// 输出 Python 删除脚本：跨平台单一格式，消除 sh/cmd 双轨。
/// 用户审查后取消 `# os.remove(...)` 注释，`python3 script.py` 执行；
/// windows 路径含 `\` 在 Python 字面量按 `\\` 转义，避免 sh 风格 `\` 歧义。
const SCRIPT_HEADER: &str = "#!/usr/bin/env python3\n\
\"\"\"tidymedia find 删除脚本：审查后取消注释 os.remove() 行后 `python3 <file>` 执行。\"\"\"\n\
import os\n\n";

pub(crate) fn render_script(
    same: &[DuplicateGroup],
    output_prefix: Option<&str>,
    sink: &mut impl Write,
) {
    if same.is_empty() {
        return;
    }
    let _ = sink.write_all(SCRIPT_HEADER.as_bytes());
    // 输入已按 size 降序（DuplicateGroup filter_and_sort 内部约定）；直接顺序遍历。
    for group in same {
        let _ = writeln!(sink, "# SIZE {}", group.size);
        // 当指定了 output_prefix 但组内**无任何**文件位于 prefix 下（用户尚未把任一份
        // 副本归档进 output），如果按原逻辑全发 active `os.remove(...)` 用户跑脚本即
        // 永久数据丢失。此时把首份保留为注释作 survivor，其余仍标记为删除，让用户
        // 至少保住一份；下方 `# SURVIVOR` 标记让审查者明白该行被特殊保护的原因。
        let any_under_prefix = output_prefix.is_some_and(|p| {
            group
                .paths
                .iter()
                .any(|path| under_prefix(path.as_str(), p))
        });
        for (idx, path) in group.paths.iter().enumerate() {
            let path_str = path.as_str();
            let escaped = escape_py_string(path_str);
            let protect = match output_prefix {
                None => true,
                Some(p) if any_under_prefix => under_prefix(path_str, p),
                // 无 path 在 prefix 下：保留首份作 survivor，其余仍删
                Some(_) => idx == 0,
            };
            if protect {
                if output_prefix.is_some() && !any_under_prefix && idx == 0 {
                    let _ = writeln!(sink, "# SURVIVOR (no copy under output)");
                }
                let _ = writeln!(sink, "# os.remove(\"{escaped}\")");
            } else {
                let _ = writeln!(sink, "os.remove(\"{escaped}\")");
            }
        }
        let _ = writeln!(sink);
    }
}

/// Python 字符串字面量转义：`\` → `\\`、`"` → `\"`。
/// 路径含 `\n` 等控制字符极罕见，转 `\xNN` 留给后续如有需要再加。
fn escape_py_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
#[path = "find_tests.rs"]
mod tests;
