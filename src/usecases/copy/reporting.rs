//! copy/move 的结构化日志与 [`CopyReport`] 构造/发射：从 `run.rs` 拆出，
//! 保持主流程编排文件在 512 行限内（文件组织规则）。

use tracing::debug;

use crate::entities::file_index::VisitStats;
use crate::usecases::report::{CopyReport, Report, ReportError, ReportSink};

/// 报告口径四开关；打包让 `finalize` / `make_report` 参数列表不再逐项透传。
#[expect(
    clippy::struct_excessive_bools,
    reason = "dry_run/remove/include_non_media/doc_only 四态互相独立（CLI flag 一比一），收敛 enum 反让调用点更绕"
)]
#[derive(Clone, Copy)]
pub(super) struct ReportFlags {
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub doc_only: bool,
}

pub(super) fn log_scan_summary(feature: &'static str, total_files: usize, stats: VisitStats) {
    debug!(
        feature,
        operation = "scan_sources",
        result = "ok",
        total_files,
        skipped_empty = stats.skipped_empty,
        skipped_unreadable = stats.skipped_unreadable,
        walker_errors = stats.walker_errors,
        "scanned source files"
    );
}

pub(super) fn log_operation_summary(
    feature: &'static str,
    total: usize,
    copied: usize,
    ignored: usize,
    failed: usize,
    flags: ReportFlags,
    stats: VisitStats,
) {
    debug!(
        feature,
        operation = "summary",
        result = summary_result(failed),
        total,
        copied,
        ignored,
        failed,
        dry_run = flags.dry_run,
        remove = flags.remove,
        include_non_media = flags.include_non_media,
        doc_only = flags.doc_only,
        skipped_empty = stats.skipped_empty,
        skipped_unreadable = stats.skipped_unreadable,
        walker_errors = stats.walker_errors,
        "operation summary"
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "finalize 与 make_report / run_copy_loop 返回项一一对应；折 struct 让唯一调用点更绕"
)]
pub(super) fn finalize(
    sink: Option<&dyn ReportSink>,
    flags: ReportFlags,
    stats: VisitStats,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
    errors_truncated: bool,
    duration_ms: u64,
) -> CopyReport {
    let report = make_report(
        flags,
        stats,
        copied,
        ignored,
        failed,
        errors,
        errors_truncated,
        duration_ms,
    );
    emit_report(sink, &report);
    report
}

// 构造 CopyReport 值对象；抽出避免参数列表过长。
// scanned = 入索引文件数（indexed）+ walker 触达但跳过的（empty/unreadable/walker_errors）。
#[expect(
    clippy::too_many_arguments,
    reason = "report 字段与 run_copy_loop 返回值一一对应；合并结构体反而在唯一调用点更绕"
)]
fn make_report(
    flags: ReportFlags,
    scan_stats: VisitStats,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
    errors_truncated: bool,
    duration_ms: u64,
) -> CopyReport {
    // indexed = copied + ignored + failed（do_copy 三态都来自已入索引的文件）。
    let indexed = copied + ignored + failed;
    let skipped_total =
        scan_stats.skipped_empty + scan_stats.skipped_unreadable + scan_stats.walker_errors;
    let scanned = indexed + usize::try_from(skipped_total).unwrap_or(usize::MAX);
    CopyReport {
        scanned,
        copied,
        ignored,
        failed,
        skipped_empty: scan_stats.skipped_empty,
        skipped_unreadable: scan_stats.skipped_unreadable,
        walker_errors: scan_stats.walker_errors,
        dry_run: flags.dry_run,
        remove: flags.remove,
        include_non_media: flags.include_non_media,
        doc_only: flags.doc_only,
        errors,
        errors_truncated,
        duration_ms,
    }
}

// 结构化日志 summary 的 result 维度值：失败计数为 0 即 "ok"，否则 "partial"。
pub(super) fn summary_result(failed: usize) -> &'static str {
    if failed == 0 { "ok" } else { "partial" }
}

// 通过注入的 sink 输出报告；None 时跳过（用 case 不知道协议与持久化细节）。
fn emit_report(sink: Option<&dyn ReportSink>, report: &CopyReport) {
    if let Some(s) = sink {
        s.write(&Report::Copy(report));
    }
}
