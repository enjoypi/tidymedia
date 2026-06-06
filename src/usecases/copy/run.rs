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
use crate::entities::common::under_prefix;
use crate::entities::file_index::{CandidateProvider, Index, VisitStats};
use crate::entities::file_info;
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

/// 测试 shim：等价于 `copy_with_sidecar(..., None)`。
/// 生产路径（dispatch）走 [`copy_with_sidecar`] 注入 P3 发现；仅测试用本简短入口。
#[cfg(test)]
pub fn copy(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
) -> common::Result<CopyReport> {
    copy_with_sidecar(
        sources,
        output,
        dry_run,
        remove,
        include_non_media,
        archive_template,
        report_sink,
        None,
    )
}

// 8 个参数源于 CLI 选项的一比一透传；与 make_report 同理 allow。
#[allow(clippy::too_many_arguments)]
pub fn copy_with_sidecar(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
    sidecar: Option<CandidateProvider>,
) -> common::Result<CopyReport> {
    let (output_loc, output_backend) = output;
    let template = archive_template.unwrap_or(&config().copy.archive_template);

    // 重叠保护（先于扫描，fail fast）。
    let output_prefix = canonical_prefix(&output_loc);
    ensure_sources_outside_output(sources, &output_prefix)?;
    let source = build_source_index(sources, &output_prefix, sidecar);

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

// 重叠保护：source ⊆ output（canonical 前缀含相等）时，dedup 会把每个源文件判为
// output 中已存在的副本，move 模式下 remove 即删除文件自身——必须 fail fast。
fn ensure_sources_outside_output(sources: &[Source], output_prefix: &str) -> common::Result<()> {
    for (loc, _) in sources {
        let src_prefix = canonical_prefix(loc);
        if under_prefix(&src_prefix, output_prefix) {
            return Err(common::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "source {src_prefix} is inside output {output_prefix}; \
                     move would treat sources as duplicates of themselves"
                ),
            )));
        }
    }
    Ok(())
}

// 扫源建索引 + 重叠剔除 + EXIF/P3 富集；拆出让 copy_with_sidecar 保持在 100 行内。
fn build_source_index(
    sources: &[Source],
    output_prefix: &str,
    sidecar: Option<CandidateProvider>,
) -> Index {
    let mut source = Index::new();
    for (loc, backend) in sources {
        source.visit_location(loc, backend);
    }
    // output ⊂ source（就地归档，如 copy /photos -o /photos/archive）：把已归档
    // 文件从 source 索引剔除，否则它们会被再次复制 / 在 move 模式下被误删。
    let excluded = source.remove_under_prefix(output_prefix);
    if excluded > 0 {
        info!(
            feature = FEATURE_COPY,
            operation = "exclude_output_subtree",
            result = "ok",
            excluded,
            output = %output_prefix,
            "excluded already-archived files under output from source index"
        );
    }
    source.parse_exif(configured_chrono_offset());
    // P3 富集：adapters 层注入的 sidecar 发现（XMP / Takeout），entities 只消费
    // 转换好的 Candidate（依赖倒置，协议细节不进 usecases）。
    if let Some(provider) = sidecar {
        source.enrich_candidates(provider);
    }
    source
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

// 重叠保护用的可比前缀：Local 走 canonicalize 消除相对路径/symlink 差异，失败
//（如 dry-run 下尚未创建的 output）回退原始路径；远端无 canonicalize，display
// 即规范形（scheme/host 不同自然不构成前缀关系）。
pub(super) fn canonical_prefix(loc: &Location) -> String {
    match loc {
        Location::Local(p) => file_info::full_path(p.as_str())
            .map_or_else(|_| p.as_str().to_string(), |fp| fp.as_str().to_string()),
        other => other.display(),
    }
}

// 通过注入的 sink 输出报告；None 时跳过（用 case 不知道协议与持久化细节）。
fn emit_report(sink: Option<&dyn ReportSink>, report: &CopyReport) {
    if let Some(s) = sink {
        s.write(&Report::Copy(report));
    }
}
