//! copy 主流程编排：扫源建索引 → 解析 EXIF → 循环 `do_copy` → 汇总报告。

use std::sync::Arc;

use chrono::FixedOffset;
use chrono::Offset;
use time::UtcOffset;
use tracing::error;
use tracing::info;
use tracing::trace;

use super::ops::do_copy;
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::file_index::{Index, VisitStats};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::report::{CopyReport, Report, ReportError, ReportSink};

/// usecase 入口的 source / output 对：把 [`Location`] 与负责该 scheme 的
/// [`Backend`] 句柄一起传入，避免内层重新解析 URI。
pub type Source = (Location, Arc<dyn Backend>);

pub(super) const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

pub(super) const FEATURE_COPY: &str = "copy";

/// [`do_copy`] 的选项包；把 bool + template 打包，规避 `clippy::too_many_arguments`。
pub struct CopyOpts<'a> {
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub template: &'a str,
}

pub(super) fn configured_offset() -> UtcOffset {
    offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界回退到 UTC，避免 panic；time crate 合法范围 ±25:59:59。
pub(super) fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

// chrono::FixedOffset 用于把 EXIF 内无时区的 NaiveDateTime 当相机本地时间解释；
// 与 time::UtcOffset 共用同一份 timezone_offset_hours 配置。
fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界（chrono::FixedOffset 合法 ±86_400 秒，即 ±24h）回退到 UTC。
pub(super) fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| chrono::Utc.fix())
}

pub fn copy(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
) -> common::Result<CopyReport> {
    let (output_loc, output_backend) = output;
    let template = archive_template.unwrap_or(&config().copy.archive_template);

    let mut source = Index::new();
    for (loc, backend) in sources {
        source.visit_location(loc, backend);
    }
    source.parse_exif(configured_chrono_offset());

    let total_files = source.files().len();
    let scan_stats = source.stats();
    info!(
        feature = FEATURE_COPY,
        operation = "scan_sources",
        result = "ok",
        total_files,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "scanned source files"
    );

    if total_files == 0 {
        // walker 触达 0 文件，但 scan_stats 仍可能含 walker_errors / skipped_*：纳入 scanned。
        let report = make_report(
            dry_run,
            remove,
            include_non_media,
            scan_stats,
            0,
            0,
            0,
            vec![],
        );
        emit_report(report_sink, &report);
        return Ok(report);
    }

    trace!(
        feature = FEATURE_COPY,
        operation = "sample_files",
        sample = ?source.some_files(10),
        "first files sample"
    );

    if !dry_run {
        output_backend.mkdir_p(&output_loc)?;
    }

    let opts = CopyOpts {
        dry_run,
        remove,
        include_non_media,
        template,
    };
    let (copied, ignored, failed, errors) =
        run_copy_loop(&source, &output_loc, &output_backend, &opts);

    let result = summary_result(failed);
    info!(
        feature = FEATURE_COPY,
        operation = "summary",
        result,
        total = total_files,
        copied,
        ignored,
        failed,
        dry_run,
        remove,
        include_non_media,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "copy operation summary"
    );

    let report = make_report(
        dry_run,
        remove,
        include_non_media,
        scan_stats,
        copied,
        ignored,
        failed,
        errors,
    );
    emit_report(report_sink, &report);
    Ok(report)
}

// 拆出循环体，让 copy() 保持在 100 行内。
fn run_copy_loop(
    source: &Index,
    output_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    opts: &CopyOpts<'_>,
) -> (usize, usize, usize, Vec<ReportError>) {
    let mut output_index = Index::new();
    output_index.visit_location(output_loc, output_backend);

    let mut copied = 0usize;
    let mut ignored = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<ReportError> = Vec::new();

    for src in source.files().values() {
        match do_copy(src, output_loc, output_backend, &mut output_index, opts) {
            Ok(true) => {
                copied += 1;
            }
            Ok(false) => {
                ignored += 1;
            }
            Err(e) => {
                failed += 1;
                let msg = e.to_string();
                error!(
                    feature = FEATURE_COPY,
                    operation = "do_copy",
                    result = "error",
                    source = %src.full_path,
                    dry_run = opts.dry_run,
                    remove = opts.remove,
                    error = %msg,
                    "copy item failed"
                );
                errors.push(ReportError {
                    path: src.full_path.to_string(),
                    message: msg,
                });
            }
        }
    }
    (copied, ignored, failed, errors)
}

// 构造 CopyReport 值对象；抽出避免参数列表过长。
// scanned = 入索引文件数（indexed）+ walker 触达但跳过的（empty/unreadable/walker_errors）。
#[allow(clippy::too_many_arguments)]
fn make_report(
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    scan_stats: VisitStats,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
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
        dry_run,
        remove,
        include_non_media,
        errors,
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
