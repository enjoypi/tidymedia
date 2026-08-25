//! copy 主流程编排：扫源建索引 → 解析 EXIF → 循环 `do_copy` → 汇总报告。

use std::sync::Arc;
use std::time::Instant;

use chrono::FixedOffset;
use time::UtcOffset;
use tracing::trace;

use super::classify::template_needs_category;
use super::copy_loop::run_copy_loop;
use super::index::{build_source_index, ensure_sources_outside_output};
use super::reporting::{ReportFlags, finalize, log_operation_summary, log_scan_summary};
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::common::canonical_prefix;
use crate::entities::file_index::{CandidateProvider, Index, TextClassifyProvider};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::config::{chrono_offset_from_hours, offset_from_hours};
use crate::usecases::report::{CopyReport, FEATURE_COPY, FEATURE_MOVE, ReportSink, feature_of};

// FEATURE_COPY / FEATURE_MOVE / feature_of 均从 usecases::report 单点导入；
// archive_template / adapters::report_sink 亦共用同一定义，避免任一漂移分裂日志聚合。
// _ 抑制未直接使用的 re-export lint（FEATURE_MOVE 只出现在 report_sink，本模块经
// feature_of 间接消费）。
const _: (&str, &str) = (FEATURE_COPY, FEATURE_MOVE);

/// usecase 入口的 source / output 对：把 [`Location`] 与负责该 scheme 的
/// [`Backend`] 句柄一起传入，避免内层重新解析 URI。
pub type Source = (Location, Arc<dyn Backend>);

pub(super) const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

/// [`super::ops::do_copy`] 的选项包；把 bool + template 打包，规避 `clippy::too_many_arguments`。
#[expect(
    clippy::struct_excessive_bools,
    reason = "dry_run/remove/include_non_media/doc_only 四态互相独立（CLI flag 一比一），收敛 enum 反让调用点更绕"
)]
pub struct CopyOpts<'a> {
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub doc_only: bool,
    pub template: &'a str,
}

pub(super) fn configured_offset() -> UtcOffset {
    offset_from_hours(config().copy.timezone_offset_hours)
}

// chrono::FixedOffset 用于把 EXIF / 文件名内无时区的 NaiveDateTime 当相机本地时间
// 解释；与 time::UtcOffset 共用同一份 timezone_offset_hours 配置。
pub(super) fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

/// 测试 shim：等价于 `copy_with_sidecar(..., doc_only=false, None)`。
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
        false,
        archive_template,
        report_sink,
        None,
        None,
    )
}

/// 解析最终归档模板：显式传入优先，否则按命令族取配置默认
/// （doc 命令用 `doc_archive_template`，含 `{category}`）。
/// `pub(crate)`：dispatch 构造分类器前需用同一口径判断模板是否消费 `{category}`。
pub(crate) fn resolved_template(archive_template: Option<&str>, doc_only: bool) -> &str {
    archive_template.unwrap_or(if doc_only {
        &config().copy.doc_archive_template
    } else {
        &config().copy.archive_template
    })
}

// 9 个参数源于 CLI 选项的一比一透传；与 make_report 同理。
#[expect(
    clippy::too_many_arguments,
    reason = "CLI 选项 + sidecar provider 一比一透传，折结构体会让 dispatch 调用点同样冗长"
)]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "dry_run/remove/include_non_media/doc_only 与 CLI flag 一比一透传，收敛 enum 反让 dispatch 调用点更绕"
)]
pub fn copy_with_sidecar(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    doc_only: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
    sidecar: Option<CandidateProvider>,
    classifier: Option<TextClassifyProvider>,
) -> common::Result<CopyReport> {
    let start = Instant::now();
    let (output_loc, output_backend) = output;
    let template = resolved_template(archive_template, doc_only);

    let output_prefix = canonical_prefix(&output_loc);
    ensure_sources_outside_output(sources, &output_prefix)?;
    let feature = feature_of(remove);
    let source = build_source_index(
        sources,
        (&output_prefix, &output_loc.display()),
        sidecar,
        // dispatch 仅在 doc_only 时构造 Some(classifier)，此处只补「模板是否
        // 消费 {category}」的单一守卫（无消费点不做工）。
        classifier.filter(|_| template_needs_category(template)),
        feature,
        include_non_media,
        doc_only,
    );

    let total_files = source.len();
    let scan_stats = source.stats();
    log_scan_summary(feature, total_files, scan_stats);

    let flags = ReportFlags {
        dry_run,
        remove,
        include_non_media,
        doc_only,
    };
    if total_files == 0 {
        return Ok(finalize(
            report_sink,
            flags,
            scan_stats,
            0,
            0,
            0,
            Vec::new(),
            false,
            crate::usecases::report::elapsed_ms(start),
        ));
    }

    execute_copy(
        &source,
        (output_loc, output_backend),
        template,
        flags,
        feature,
        report_sink,
        start,
    )
}

// copy_with_sidecar 的非空源主路径：mkdir → run_copy_loop → 日志 → finalize。
// 拆出让入口函数保持在 64 行限内（P0 §10）。
fn execute_copy(
    source: &Index,
    output: Source,
    template: &str,
    flags: ReportFlags,
    feature: &'static str,
    report_sink: Option<&dyn ReportSink>,
    start: Instant,
) -> common::Result<CopyReport> {
    let (output_loc, output_backend) = output;
    trace!(
        feature,
        operation = "sample_files",
        sample = ?source.some_files(10),
        "first files sample"
    );

    if !flags.dry_run {
        output_backend.mkdir_p(&output_loc)?;
    }

    let opts = CopyOpts {
        dry_run: flags.dry_run,
        remove: flags.remove,
        include_non_media: flags.include_non_media,
        doc_only: flags.doc_only,
        template,
    };
    let (copied, ignored, failed, errors, errors_truncated) =
        run_copy_loop(source, &output_loc, &output_backend, &opts);

    let scan_stats = source.stats();
    log_operation_summary(
        feature,
        source.len(),
        copied,
        ignored,
        failed,
        flags,
        scan_stats,
    );
    Ok(finalize(
        report_sink,
        flags,
        scan_stats,
        copied,
        ignored,
        failed,
        errors,
        errors_truncated,
        crate::usecases::report::elapsed_ms(start),
    ))
}
