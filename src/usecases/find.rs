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

// debug!/error! 宏在不同 instantiation 间会产生重复的内部 region；用例入口本身的逻辑
// 已经被各种集成测试覆盖。整体标 coverage(off) 让严格覆盖率统计稳定。
//
// 返回值：完整的 FindReport（scanned = 全部入索引的文件数；bytes_read 来自 Index 累计；
// groups 为 DuplicateGroup 列表）。dispatch 层可直接落 JSON 而无需重新统计。
//
// # Errors
//
// output 路径不存在或不是目录时返回 `Err`：空报告 + exit 0 与"无重复"不可区分，
// 会误导基于退出码/空脚本做删除决策的调用方。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn find_duplicates(
    secure: bool,
    sources: Vec<Source>,
    output: Option<&Source>,
) -> common::Result<FindReport> {
    let mut index = file_index::Index::new();

    if let Some((loc, backend)) = output {
        let is_dir = backend
            .metadata(loc)
            .is_ok_and(|m| m.kind == EntryKind::Dir);
        if !is_dir {
            error!(
                feature = FEATURE_FIND,
                operation = "validate_output",
                result = "not_a_directory",
                output = %loc.display(),
                "output path is not a directory"
            );
            return Err(common::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("output path is not a directory: {}", loc.display()),
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

    render_script(
        &groups,
        prefix_owned.as_deref(),
        comment(),
        rm(),
        &mut std::io::stdout(),
    );

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

// 上方 is_dir 断言已经过滤掉非目录；到这里 output 必然是 (Location, Backend) 形态。
// Local 走 full_path canonicalize（兼容旧 prefix 字符串语义）；远端走 Location::display。
// expect 的 panic 边永远不被触发，被 LLVM 当作 region miss，故抽出后标 coverage(off)。
#[cfg_attr(coverage_nightly, coverage(off))]
fn compute_output_prefix(output: Option<&Source>) -> Option<String> {
    output.map(|(loc, _)| match loc {
        Location::Local(p) => file_info::full_path(p.as_str())
            .expect("output path validated as directory above")
            .as_str()
            .to_string(),
        other => other.display(),
    })
}

// `\r` 仅 Windows：让生成的脚本在 cmd.exe 用 CRLF 行尾，Linux/macOS sh 走 LF。
// 旧实现硬编码 `\r` 在所有平台 → Linux 下输出 CRLF，下游 `tidymedia find | sh`
// 时 shell 把尾随 CR 当路径字符的一部分 → `rm "foo.jpg"^M` 触发
// "rm: cannot remove 'foo.jpg<CR>': No such file or directory"，所有删除静默失败。
#[cfg(target_os = "windows")]
pub(crate) const SCRIPT_LINE_TAIL: &str = "\r";
#[cfg(not(target_os = "windows"))]
pub(crate) const SCRIPT_LINE_TAIL: &str = "";

pub(crate) fn render_script(
    same: &[DuplicateGroup],
    output_prefix: Option<&str>,
    comment_token: &str,
    rm_token: &str,
    sink: &mut impl Write,
) {
    // 输入已按 size 降序（DuplicateGroup filter_and_sort 内部约定）；直接顺序遍历。
    for group in same {
        let _ = writeln!(sink, "{comment_token}SIZE {}{SCRIPT_LINE_TAIL}", group.size);
        for path in &group.paths {
            let path_str = path.as_str();
            let starts = output_prefix.is_some_and(|p| under_prefix(path_str, p));
            if output_prefix.is_some() && !starts {
                let _ = writeln!(sink, "{rm_token} \"{path}\"{SCRIPT_LINE_TAIL}");
            } else {
                let _ = writeln!(
                    sink,
                    "{comment_token}{rm_token} \"{path}\"{SCRIPT_LINE_TAIL}"
                );
            }
        }
        let _ = writeln!(sink);
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn comment() -> &'static str {
    ":"
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn comment() -> &'static str {
    "#"
}

#[cfg(target_os = "windows")]
pub(crate) fn rm() -> &'static str {
    "DEL"
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn rm() -> &'static str {
    "rm"
}

#[cfg(test)]
#[path = "find_tests.rs"]
mod tests;
