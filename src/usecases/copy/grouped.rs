use std::sync::Arc;

use dashmap::DashSet;
use tracing::info;

use super::copy_loop::run_copy_loop;
use super::index::{build_source_index, scan_output, source_skip_extensions};
use super::reporting::{finalize, log_operation_summary};
use super::run::CopyJob;
use crate::entities::common;
use crate::entities::file_index::{Admission, Index, VisitStats};
use crate::entities::file_info::Info;
use crate::entities::memory::Ledger;
use crate::usecases::memory::{name_cost, output_cost, source_cost};
use crate::usecases::report::{CopyReport, ReportError, extend_errors_capped};
use crate::usecases::retained::{EntryCost, memory_error};
use crate::usecases::retained_plan::{Plan, plan_groups};

#[derive(Default)]
struct Totals {
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
    errors_truncated: bool,
    bytes_read: u64,
    unreadable: u64,
    indexed: usize,
}

fn walk(job: &CopyJob<'_>, make: &dyn Fn(EntryCost) -> Arc<dyn Admission>) -> VisitStats {
    let mut sources = Index::new().with_admission(make(source_cost()));
    let skip_ext = source_skip_extensions(job.flags.include_non_media, job.flags.doc_only);
    for (loc, backend) in job.sources {
        sources.visit_location_skipping(loc, backend, skip_ext);
    }
    drop(scan_output(
        job,
        Index::new().with_admission(make(output_cost())),
    ));
    sources.stats()
}

pub(super) fn copy_grouped(
    job: &CopyJob<'_>,
    ledger: &'static Ledger,
) -> common::Result<CopyReport> {
    let names_cost = if job.flags.dry_run {
        name_cost()
    } else {
        EntryCost {
            base: 0,
            path_copies: 0,
        }
    };
    let planned = plan_groups(ledger.capacity(), names_cost, &|make| walk(job, make))
        .map_err(memory_error)?;
    let shared = planned.shared;
    info!(
        feature = job.feature,
        operation = "plan_groups",
        result = "grouped",
        groups = planned.plan.groups,
        "input exceeds the memory budget; processing in size groups"
    );
    if !job.flags.dry_run {
        job.output_backend.mkdir_p(&job.output_loc)?;
    }
    let totals = run_groups(job, &planned.plan);
    let stats = VisitStats {
        skipped_unreadable: shared.skipped_unreadable + totals.unreadable,
        ..shared
    };
    log_operation_summary(
        job.feature,
        totals.indexed,
        totals.copied,
        totals.ignored,
        totals.failed,
        job.flags,
        stats,
    );
    finalize(
        job.report_sink,
        job.flags,
        stats,
        totals.copied,
        totals.ignored,
        totals.failed,
        totals.errors,
        totals.errors_truncated,
        totals.bytes_read,
        crate::usecases::report::elapsed_ms(job.start),
    )
}

fn run_groups(job: &CopyJob<'_>, plan: &Arc<Plan>) -> Totals {
    let names = Arc::new(DashSet::new());
    let mut totals = Totals::default();
    let mut carry: Vec<Info> = Vec::new();
    for group in 0..plan.groups {
        let output = Index::new().with_admission(plan.output_filter(group));
        let output_index = scan_output(
            job,
            if job.flags.dry_run {
                output.sharing_names(Arc::clone(&names))
            } else {
                output
            },
        );
        carry.drain(..).for_each(|info| output_index.add(info));
        let source = build_source_index(job, Index::new().with_admission(plan.filter(group)));
        run_group(job, &source, &output_index, &mut totals);
        if job.flags.dry_run {
            carry = output_index
                .into_infos()
                .into_iter()
                .filter(|info| info.is_staged() && plan.is_chunked(info))
                .collect();
        }
    }
    totals
}

fn run_group(job: &CopyJob<'_>, source: &Index, output_index: &Index, totals: &mut Totals) {
    totals.unreadable += source.stats().skipped_unreadable;
    totals.indexed += source.len();
    totals.bytes_read += source.bytes_read() + output_index.bytes_read();
    let (copied, ignored, failed, errors, truncated) = run_copy_loop(
        source,
        output_index,
        false,
        (&job.output_loc, &job.output_backend),
        &job.opts(),
    );
    totals.copied += copied;
    totals.ignored += ignored;
    totals.failed += failed;
    extend_errors_capped(
        &mut totals.errors,
        &mut totals.errors_truncated,
        errors,
        truncated,
    );
}

#[cfg(test)]
#[path = "grouped_tests.rs"]
mod tests;
