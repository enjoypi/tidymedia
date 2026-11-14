use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use tracing::debug;
use tracing::error;

use crate::entities::backend::EntryKind;
use crate::entities::common;
use crate::entities::common::under_prefix;
use crate::entities::file_index;
use crate::entities::file_index::DuplicateGroup;

use super::copy::Source;
use super::memory::{retained, source_cost};
use super::report::{DuplicateGroupReport, FEATURE_FIND, FindReport};
use super::retained::{Charging, EntryCost, memory_error};
use super::retained_plan::plan_groups;
use super::spill::{Listed, Spill};
use crate::entities::memory::Ledger;

//
// # Errors
//
// output 路径不存在或不是目录时返回 `Err`：空报告 + exit 0 与"无重复"不可区分，
// 会误导基于退出码/空脚本做删除决策的调用方。
pub(crate) fn find_duplicates(
    secure: bool,
    sources: &[Source],
    output: Option<&Source>,
    script_sink: &mut dyn Write,
) -> common::Result<FindReport> {
    let start = Instant::now();
    let mut index = file_index::Index::new();

    if let Some(output) = output {
        validate_output(output)?;
    }

    let ledger = retained();
    ledger.reset();
    index = index_sources(
        sources,
        index.with_admission(Arc::new(Charging {
            ledger,
            cost: source_cost(),
        })),
    );
    if index.overflowed() {
        drop(index);
        let found = find_grouped(secure, sources, ledger)?;
        return finish(found, output, script_sink, start);
    }

    let scan_stats = index.stats();
    let found = Found {
        scanned: index.touched_count(),
        bytes_read: index.bytes_read(),
        groups: reports(if secure {
            index.search_same()
        } else {
            index.fast_search_same()
        })
        .into(),
    };
    debug!(
        feature = FEATURE_FIND,
        operation = "scan_complete",
        result = "ok",
        secure,
        files = found.scanned,
        similar_files = index.similar_bucket_count(),
        bytes_read = found.bytes_read,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "index built"
    );
    finish(found, output, script_sink, start)
}

fn validate_output((loc, backend): &Source) -> common::Result<()> {
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
    Ok(())
}

struct Found {
    scanned: usize,
    bytes_read: u64,
    groups: Listed<DuplicateGroupReport>,
}

fn reports(groups: Vec<DuplicateGroup>) -> Vec<DuplicateGroupReport> {
    groups.into_iter().map(DuplicateGroupReport::from).collect()
}

fn index_sources(sources: &[Source], mut index: file_index::Index) -> file_index::Index {
    for (loc, backend) in sources {
        index.visit_location(loc, backend);
    }
    index
}

fn find_grouped(
    secure: bool,
    sources: &[Source],
    ledger: &'static Ledger,
) -> common::Result<Found> {
    let planned = plan_groups(
        ledger.capacity(),
        EntryCost {
            base: 0,
            path_copies: 0,
        },
        &|make| {
            index_sources(
                sources,
                file_index::Index::new().with_admission(make(source_cost())),
            )
            .stats()
        },
    )
    .map_err(memory_error)?;
    let plan = planned.plan;
    let shared = planned.shared.skipped_total();
    let mut found = Found {
        scanned: usize::try_from(shared).unwrap_or(usize::MAX),
        bytes_read: 0,
        groups: Listed::default(),
    };
    let mut spill = Spill::default();
    (0..plan.groups)
        .try_for_each(|group| {
            let index = index_sources(
                sources,
                file_index::Index::new().with_admission(plan.filter(group)),
            );
            found.scanned +=
                index.len() + usize::try_from(index.stats().skipped_unreadable).unwrap_or(0);
            found.bytes_read += index.bytes_read();
            spill.push_run(&reports(if secure {
                index.search_same()
            } else {
                index.fast_search_same()
            }))
        })
        .map_err(common::Error::Io)
        .map(|()| Found {
            groups: Listed::spilled(spill),
            ..found
        })
}

fn finish(
    found: Found,
    output: Option<&Source>,
    script_sink: &mut dyn Write,
    start: Instant,
) -> common::Result<FindReport> {
    let Found {
        scanned,
        bytes_read,
        groups,
    } = found;
    debug!(
        feature = FEATURE_FIND,
        operation = "search_same",
        result = "ok",
        groups = groups.len(),
        "duplicate groups discovered"
    );

    let prefix_owned = compute_output_prefix(output);

    render_script(&groups, prefix_owned.as_deref(), script_sink)?;

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
        groups,
        duration_ms: super::report::elapsed_ms(start),
    })
}

#[must_use]
fn compute_output_prefix(output: Option<&Source>) -> Option<String> {
    output.map(|(loc, _)| common::canonical_prefix(loc))
}

/// 输出 Python 删除脚本：跨平台单一格式，消除 sh/cmd 双轨。
/// 用户审查后取消 `# os.remove(...)` 注释，`python3 script.py` 执行；
/// windows 路径含 `\` 在 Python 字面量按 `\\` 转义，避免 sh 风格 `\` 歧义。
const SCRIPT_HEADER: &str = "#!/usr/bin/env python3\n\
\"\"\"tidymedia find 删除脚本：审查后取消注释 os.remove() 行后 `python3 <file>` 执行。\"\"\"\n\
import os\n\n";

pub(crate) fn render_script(
    groups: &Listed<DuplicateGroupReport>,
    output_prefix: Option<&str>,
    sink: &mut dyn Write,
) -> std::io::Result<()> {
    let mut started = false;
    groups.for_each(&mut |group| {
        if !started {
            started = true;
            sink.write_all(SCRIPT_HEADER.as_bytes())?;
        }
        render_group(group, output_prefix, sink)
    })
}

fn render_group(
    group: &DuplicateGroupReport,
    output_prefix: Option<&str>,
    sink: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(sink, "# SIZE {}", group.size)?;
    let any_under_prefix = output_prefix.is_some_and(|p| {
        group
            .paths
            .iter()
            .any(|path| under_prefix(&common::key_from_display(path), p))
    });
    for (idx, path) in group.paths.iter().enumerate() {
        let escaped = escape_py_string(path);
        let (protect, is_survivor) = decide_protect(idx, output_prefix, path, any_under_prefix);
        if protect {
            if is_survivor {
                writeln!(sink, "# SURVIVOR (no copy under output)")?;
            }
            writeln!(sink, "# os.remove(\"{escaped}\")")?;
        } else {
            writeln!(sink, "os.remove(\"{escaped}\")")?;
        }
    }
    writeln!(sink)
}

/// 一条 `os.remove` 应否被注释（protect）以及是否是 survivor（组内无 path 在
/// prefix 下的首份保留）。抽独立 fn 让 branch counter 收敛到单 codegen instance，
/// 消除 `render_script` 在 lib unit + subprocess 双 instance 下的 phantom miss。
fn decide_protect(
    idx: usize,
    output_prefix: Option<&str>,
    path_str: &str,
    any_under_prefix: bool,
) -> (bool, bool) {
    match output_prefix {
        None => (true, false),
        Some(p) if any_under_prefix => {
            (under_prefix(&common::key_from_display(path_str), p), false)
        }
        // 无 path 在 prefix 下：保留首份作 survivor，其余仍删
        Some(_) => (idx == 0, idx == 0),
    }
}

fn escape_py_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 其余 ASCII 控制字符（NUL / BEL / VT / FF / DEL 等）走 \xNN 兜底。
            c if (c as u32) < 0x20 || c == '\x7f' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "find_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "find_grouped_tests.rs"]
mod grouped_tests;
