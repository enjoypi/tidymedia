use std::sync::Arc;
use std::time::Instant;

use chrono::FixedOffset;
use time::UtcOffset;
use tracing::trace;

use super::classify::template_needs_category;
use super::copy_loop::run_copy_loop;
use super::grouped::copy_grouped;
use super::index::{build_source_index, ensure_sources_outside_output, scan_output};
use super::reporting::{ReportFlags, finalize, log_operation_summary, log_scan_summary};
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::common::canonical_prefix;
use crate::entities::file_index::{
    Admission, CandidateProvider, Index, TextClassifyProvider, VisitStats,
};
use crate::entities::memory::Ledger;
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::config::{chrono_offset_from_hours, offset_from_hours};
use crate::usecases::memory::{output_cost, retained, source_cost};
use crate::usecases::report::{CopyReport, FEATURE_COPY, FEATURE_MOVE, ReportSink, feature_of};
use crate::usecases::retained::{Charging, EntryCost};

const _: (&str, &str) = (FEATURE_COPY, FEATURE_MOVE);

pub type Source = (Location, Arc<dyn Backend>);

pub(super) const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

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

pub(super) fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

pub(crate) fn resolved_template(archive_template: Option<&str>, doc_only: bool) -> &str {
    archive_template.unwrap_or(if doc_only {
        &config().copy.doc_archive_template
    } else {
        &config().copy.archive_template
    })
}

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
    let classifier = classifier.filter(|_| template_needs_category(template));
    let job = CopyJob {
        sources,
        output_loc,
        output_backend,
        output_prefix,
        sidecar,
        classifier: classifier.as_ref(),
        flags: ReportFlags {
            dry_run,
            remove,
            include_non_media,
            doc_only,
        },
        template,
        feature: feature_of(remove),
        report_sink,
        start,
    };
    let ledger = retained();
    ledger.reset();
    let output_index = scan_output(
        &job,
        Index::new().with_admission(charging(ledger, output_cost())),
    );
    if !output_index.overflowed() {
        let source = build_source_index(
            &job,
            Index::new().with_admission(charging(ledger, source_cost())),
        );
        if !source.overflowed() {
            let authoritative = output_index.stats() == VisitStats::default();
            return execute_copy(&job, &source, &output_index, authoritative);
        }
    }
    copy_grouped(&job, ledger)
}

pub(super) struct CopyJob<'a> {
    pub sources: &'a [Source],
    pub output_loc: Location,
    pub output_backend: Arc<dyn Backend>,
    pub output_prefix: String,
    pub sidecar: Option<CandidateProvider>,
    pub classifier: Option<&'a TextClassifyProvider>,
    pub flags: ReportFlags,
    pub template: &'a str,
    pub feature: &'static str,
    pub report_sink: Option<&'a dyn ReportSink>,
    pub start: Instant,
}

impl CopyJob<'_> {
    pub(super) fn opts(&self) -> CopyOpts<'_> {
        CopyOpts {
            dry_run: self.flags.dry_run,
            remove: self.flags.remove,
            include_non_media: self.flags.include_non_media,
            doc_only: self.flags.doc_only,
            template: self.template,
        }
    }
}

fn charging(ledger: &'static Ledger, cost: EntryCost) -> Arc<dyn Admission> {
    Arc::new(Charging { ledger, cost })
}

fn execute_copy(
    job: &CopyJob<'_>,
    source: &Index,
    output_index: &Index,
    authoritative: bool,
) -> common::Result<CopyReport> {
    let feature = job.feature;
    let scan_stats = source.stats();
    log_scan_summary(feature, source.len(), scan_stats);
    let bytes_read = || source.bytes_read() + output_index.bytes_read();
    if source.is_empty() {
        return finalize(
            job.report_sink,
            job.flags,
            scan_stats,
            0,
            0,
            0,
            Vec::new(),
            false,
            bytes_read(),
            crate::usecases::report::elapsed_ms(job.start),
        );
    }
    trace!(
        feature,
        operation = "sample_files",
        sample = ?source.some_files(10),
        "first files sample"
    );
    if !job.flags.dry_run {
        job.output_backend.mkdir_p(&job.output_loc)?;
    }
    let (copied, ignored, failed, errors, errors_truncated) = run_copy_loop(
        source,
        output_index,
        authoritative,
        (&job.output_loc, &job.output_backend),
        &job.opts(),
    );
    log_operation_summary(
        feature,
        source.len(),
        copied,
        ignored,
        failed,
        job.flags,
        scan_stats,
    );
    finalize(
        job.report_sink,
        job.flags,
        scan_stats,
        copied,
        ignored,
        failed,
        errors,
        errors_truncated,
        bytes_read(),
        crate::usecases::report::elapsed_ms(job.start),
    )
}
