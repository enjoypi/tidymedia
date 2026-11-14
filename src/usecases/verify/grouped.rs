use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::info;

use super::candidates::variant_key;
use super::content_diff::basename;
use super::exif_tsv::ExifRow;
use super::output_library::OutputLibrary;
use super::report::VerifyReport;
use super::{VerifyJob, evaluate_all, fold_evaluated, scan_sources};
use crate::entities::backend::{Entry, EntryKind};
use crate::entities::common;
use crate::entities::file_index::{Admission, Admit, Index};
use crate::entities::file_info::Info;
use crate::entities::memory::Ledger;
use crate::usecases::memory::{output_cost, source_cost};
use crate::usecases::report::FEATURE_VERIFY;
use crate::usecases::retained::{EntryCost, memory_error};
use crate::usecases::retained_plan::{GroupFilter, plan_groups};
use crate::usecases::spill::{Listed, Spill};

const NAME_MATCH_HEADROOM_DIVISOR: u64 = 4;

struct OutputFilter {
    group: Arc<GroupFilter>,
    names: HashSet<String>,
}

impl OutputFilter {
    fn named(&self, path: &str) -> bool {
        let name = basename(path).to_ascii_lowercase();
        self.names.contains(&name) || variant_key(&name).is_some_and(|k| self.names.contains(&k))
    }
}

impl Admission for OutputFilter {
    fn admit(&self, entry: &Entry) -> Admit {
        if self.group.owns_entry(entry) || self.named(&entry.location.display()) {
            Admit::Take
        } else {
            Admit::Skip
        }
    }

    fn counts_shared_stats(&self) -> bool {
        false
    }

    fn keep_info(&self, info: &Info) -> bool {
        self.named(info.full_path.as_str()) || self.group.owns_info(info)
    }
}

fn walk(
    job: &VerifyJob<'_>,
    make: &dyn Fn(EntryCost) -> Arc<dyn Admission>,
) -> crate::entities::file_index::VisitStats {
    let mut sources = Index::new().with_admission(make(source_cost()));
    let skip_ext = crate::usecases::copy::source_skip_extensions(job.include_non_media, false);
    for (loc, backend) in job.sources {
        sources.visit_location_skipping(loc, backend, skip_ext);
    }
    let (loc, backend) = job.output;
    if backend
        .metadata(loc)
        .is_ok_and(|m| m.kind == EntryKind::Dir)
    {
        Index::new()
            .with_admission(make(output_cost()))
            .visit_location(loc, backend);
    }
    sources.stats()
}

pub(super) fn run(
    job: &VerifyJob<'_>,
    tsv: &HashMap<&str, &ExifRow>,
    report: &mut VerifyReport,
    ledger: &'static Ledger,
) -> common::Result<()> {
    let capacity = ledger.capacity() - ledger.capacity() / NAME_MATCH_HEADROOM_DIVISOR;
    let names = EntryCost {
        base: 0,
        path_copies: 0,
    };
    let planned = plan_groups(capacity, names, &|make| walk(job, make)).map_err(memory_error)?;
    info!(
        feature = FEATURE_VERIFY,
        operation = "plan_groups",
        result = "grouped",
        groups = planned.plan.groups,
        "input exceeds the memory budget; verifying in size groups"
    );
    report.scanned = usize::try_from(planned.shared.skipped_total()).unwrap_or(usize::MAX);
    let mut spill = Spill::default();
    (0..planned.plan.groups)
        .try_for_each(|group| {
            let source = scan_sources(job, planned.plan.filter(group));
            report.scanned += source.len()
                + usize::try_from(source.stats().skipped_unreadable).unwrap_or(usize::MAX);
            let names = source
                .iter()
                .map(|kv| basename(kv.key().as_str()).to_ascii_lowercase())
                .collect();
            let (out, _) = OutputLibrary::scan(
                job.output,
                Arc::new(OutputFilter {
                    group: planned.plan.output_filter(group),
                    names,
                }),
            );
            let evaluated = evaluate_all(job, source, &out, tsv);
            spill.push_run(&fold_evaluated(report, evaluated))
        })
        .map_err(common::Error::Io)
        .map(|()| report.entries = Listed::spilled(spill))
}

#[cfg(test)]
#[path = "grouped_tests.rs"]
mod tests;
