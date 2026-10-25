use std::collections::HashMap;

use chrono::FixedOffset;

use crate::entities::file_info::Info;
use crate::entities::media_time::ConflictKind;

use super::bucket::actual_bucket;
use super::content_diff::{OutputLibrary, verdict_for};
use super::diagnose::{DiagnoseInput, fix_suggestion, patterns};
use super::exif_tsv::{ExifRow, expected_bucket};
use super::filename_hint::parse_path_date_bucket;
use super::report::VerifyEntry;
use super::{EntryParts, build_entry, lookup_tsv, strip_source_root};

pub(super) struct EvalCtx<'a> {
    pub(super) offset: FixedOffset,
    pub(super) tz: i8,
    pub(super) tsv: &'a HashMap<&'a str, &'a ExifRow>,
    pub(super) roots: &'a [String],
    pub(super) out: &'a OutputLibrary,
    pub(super) phash_max: u8,
    pub(super) max_bytes: u64,
}

pub(super) struct Evaluated {
    pub(super) entry: VerifyEntry,
    pub(super) decision_failed: bool,
}

pub(super) fn evaluate(info: &Info, ctx: &EvalCtx<'_>) -> Evaluated {
    let path = info.full_path.as_str();
    let decision = info.media_time_decision(ctx.offset);
    let bucket = decision.as_ref().map(|d| actual_bucket(d.utc, ctx.offset));
    let exif = lookup_tsv(ctx.tsv, path);
    let (exp, from_label) = exif.map_or((None, None), |r| expected_bucket(r, ctx.tz));
    let mismatch = match (exp.as_deref(), bucket.as_deref()) {
        (Some(e), Some(b)) => e != b,
        _ => false,
    };
    let filename_bucket = parse_path_date_bucket(&strip_source_root(path, ctx.roots));
    let verdict = verdict_for(info, ctx.out, ctx.phash_max, ctx.max_bytes);
    let conflicts: Vec<ConflictKind> = decision
        .as_ref()
        .map_or_else(Vec::new, |d| d.conflicts.iter().map(|c| c.kind).collect());
    let diag = DiagnoseInput {
        actual_bucket: bucket.as_deref(),
        exp_bucket: exp.as_deref(),
        exif_from: from_label.as_deref(),
        filename_bucket: filename_bucket.as_deref(),
        conflicts: &conflicts,
        duplicate_verdict: &verdict,
        mismatch,
    };
    let matched_patterns = patterns(&diag);
    let suggestion = fix_suggestion(&diag);
    let entry = build_entry(EntryParts {
        path,
        decision: decision.as_ref(),
        actual_bucket: bucket,
        exif_exp_bucket: exp,
        mismatch,
        exif_from: from_label,
        exif_make: exif.map(|r| r.make.clone()),
        exif_model: exif.map(|r| r.model.clone()),
        filename_bucket,
        duplicate_verdict: verdict,
        patterns: matched_patterns,
        fix_suggestion: suggestion,
    });
    Evaluated {
        entry,
        decision_failed: decision.is_none(),
    }
}
