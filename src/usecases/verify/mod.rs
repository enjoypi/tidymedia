//! verify use case：把「tidy-verify skill」的确定性对账业务内化为子命令。
//! 流程：时间决策与桶对账 → 文件名/目录段提示 → 内容像素比对 → pattern 诊断。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::debug;
use tracing::warn;

use crate::entities::common;
use crate::entities::file_index;
use crate::entities::file_info::Info;
use crate::entities::media_time::MediaTimeDecision;
use crate::entities::memory::Ledger;
use crate::usecases::memory::{output_cost, retained, source_cost};
use crate::usecases::report::elapsed_ms;
use crate::usecases::retained::Charging;

use super::copy::Source;
use super::report::FEATURE_VERIFY;
use super::verify::evaluate::{EvalCtx, Evaluated, evaluate};
use super::verify::exif_tsv::{ExifRow, normalize_sep, parse_tsv};
use super::verify::output_library::OutputLibrary;
pub use report::{VerifyEntry, VerifyReport};
pub(crate) use summary::write_summary;

pub(crate) mod bucket;
pub(crate) mod candidates;
pub(crate) mod content_diff;
#[path = "content_diff_io.rs"]
pub(crate) mod content_diff_io;
pub(crate) mod content_features;
pub(crate) mod diagnose;
mod evaluate;
pub(crate) mod exif_tsv;
pub(crate) mod filename_hint;
mod grouped;
pub(crate) mod output_library;
pub(crate) mod phash;
pub(crate) mod report;
mod rotated_view;
pub(crate) mod summary;
#[path = "verify_io.rs"]
mod verify_io;

pub(crate) const DEFAULT_PHASH_MAX: u8 = 20;

struct VerifyJob<'a> {
    sources: &'a [Source],
    output: &'a Source,
    phash_max: u8,
    include_non_media: bool,
    offset: chrono::FixedOffset,
    tz: i8,
    roots: Vec<String>,
}

pub(crate) fn verify(
    sources: &[Source],
    output: &Source,
    phash_max: u8,
    include_non_media: bool,
    exif_tsv: Option<&Path>,
) -> common::Result<VerifyReport> {
    let start = Instant::now();
    let ledger = retained();
    ledger.reset();
    let tsv_reader = exif_tsv.filter(|path| *path == Path::new("-")).map(|path| {
        let path = path.to_path_buf();
        std::thread::spawn(move || load_tsv(Some(path.as_path())))
    });
    let tz = crate::usecases::config::config().copy.timezone_offset_hours;
    let job = VerifyJob {
        sources,
        output,
        phash_max,
        include_non_media,
        offset: crate::usecases::config::chrono_offset_from_hours(tz),
        tz,
        roots: sources.iter().map(|(loc, _)| loc.display()).collect(),
    };
    let single = single_pass(&job, ledger);
    let tsv_start = Instant::now();
    let tsv_rows = join_tsv(tsv_reader, exif_tsv);
    let tsv = index_tsv(&tsv_rows);
    log_stage("tsv_wait", tsv_start);
    let mut report = VerifyReport {
        include_non_media,
        dry_run: true,
        ..Default::default()
    };
    match single {
        Some((source, out)) => {
            report.scanned = source.touched_count();
            let evaluated = evaluate_all(&job, source, &out, &tsv);
            report.entries = fold_evaluated(&mut report, evaluated).into();
        }
        None => grouped::run(&job, &tsv, &mut report, ledger)?,
    }
    report.duration_ms = elapsed_ms(start);
    Ok(report)
}

fn single_pass(
    job: &VerifyJob<'_>,
    ledger: &'static Ledger,
) -> Option<(file_index::Index, OutputLibrary)> {
    let source = scan_sources(
        job,
        Arc::new(Charging {
            ledger,
            cost: source_cost(),
        }),
    );
    if source.overflowed() {
        return None;
    }
    let output_start = Instant::now();
    let (out, overflowed) = OutputLibrary::scan(
        job.output,
        Arc::new(Charging {
            ledger,
            cost: output_cost(),
        }),
    );
    log_stage("output_walk", output_start);
    (!overflowed).then_some((source, out))
}

fn evaluate_all(
    job: &VerifyJob<'_>,
    source: file_index::Index,
    out: &OutputLibrary,
    tsv: &HashMap<&str, &ExifRow>,
) -> Vec<Evaluated> {
    let infos: Vec<Info> = source
        .into_infos()
        .into_iter()
        .filter(|i| job.include_non_media || i.is_media())
        .collect();
    let prepare_start = Instant::now();
    out.prepare_exact(&infos);
    log_stage("output_prepare", prepare_start);
    let ctx = EvalCtx {
        offset: job.offset,
        tz: job.tz,
        tsv,
        roots: &job.roots,
        out,
        phash_max: job.phash_max,
        max_bytes: crate::usecases::config::config().verify.max_image_bytes,
    };
    let evaluate_start = Instant::now();
    let evaluated = infos.par_iter().map(|i| evaluate(i, &ctx)).collect();
    log_stage("evaluate", evaluate_start);
    evaluated
}

fn scan_sources(
    job: &VerifyJob<'_>,
    admission: Arc<dyn file_index::Admission>,
) -> file_index::Index {
    let walk_start = Instant::now();
    let skip_ext = super::copy::source_skip_extensions(job.include_non_media, false);
    let mut index = file_index::Index::new().with_admission(admission);
    for (loc, backend) in job.sources {
        index.visit_location_skipping(loc, backend, skip_ext);
    }
    log_stage("source_walk", walk_start);
    if index.overflowed() {
        return index;
    }
    let exif_start = Instant::now();
    index.parse_exif(job.offset, job.include_non_media, false);
    log_stage("source_exif", exif_start);
    debug!(
        feature = FEATURE_VERIFY,
        operation = "scan_complete",
        result = "ok",
        files = index.len(),
        "verify source index built"
    );
    index
}

fn join_tsv(
    reader: Option<std::thread::JoinHandle<Vec<ExifRow>>>,
    fallback: Option<&Path>,
) -> Vec<ExifRow> {
    match reader {
        Some(handle) => {
            if let Ok(rows) = handle.join() {
                rows
            } else {
                warn!(
                    feature = FEATURE_VERIFY,
                    operation = "load_exif_tsv",
                    result = "error",
                    error = "reader thread panicked",
                    "exif tsv reader failed; cross-check disabled"
                );
                Vec::new()
            }
        }
        None => load_tsv(fallback),
    }
}

fn log_stage(operation: &'static str, start: Instant) {
    debug!(
        feature = FEATURE_VERIFY,
        operation,
        result = "ok",
        elapsed_ms = elapsed_ms(start),
        "verify stage complete"
    );
}

fn fold_evaluated(report: &mut VerifyReport, mut evaluated: Vec<Evaluated>) -> Vec<VerifyEntry> {
    let fold_start = Instant::now();
    evaluated.sort_by(|a, b| a.entry.source_path.cmp(&b.entry.source_path));
    let mut entries = Vec::with_capacity(evaluated.len());
    for ev in evaluated {
        report.compared += 1;
        if ev.decision_failed {
            report.decision_failed += 1;
        }
        if ev.entry.mismatch {
            report.mismatched += 1;
        }
        for p in &ev.entry.patterns {
            *report.pattern_counts.entry(p.clone()).or_insert(0) += 1;
        }
        entries.push(ev.entry);
    }
    log_stage("fold", fold_start);
    entries
}

struct EntryParts<'a> {
    path: &'a str,
    decision: Option<&'a MediaTimeDecision>,
    actual_bucket: Option<String>,
    exif_exp_bucket: Option<String>,
    mismatch: bool,
    exif_from: Option<String>,
    exif_make: Option<String>,
    exif_model: Option<String>,
    filename_bucket: Option<String>,
    duplicate_verdict: String,
    patterns: Vec<String>,
    fix_suggestion: Option<String>,
}

fn build_entry(p: EntryParts<'_>) -> VerifyEntry {
    let decision = p.decision;
    VerifyEntry {
        source_path: p.path.to_string(),
        actual_bucket: p.actual_bucket.unwrap_or_default(),
        exif_exp_bucket: p.exif_exp_bucket,
        exif_from: p.exif_from,
        exif_make: p.exif_make,
        exif_model: p.exif_model,
        filename_bucket: p.filename_bucket,
        duplicate_verdict: p.duplicate_verdict,
        patterns: p.patterns,
        fix_suggestion: p.fix_suggestion,
        mismatch: p.mismatch,
        chosen_priority: decision
            .map_or_else(|| "(none)".to_owned(), |d| format!("{:?}", d.priority)),
        chosen_source: decision.map_or_else(|| "(none)".to_owned(), |d| format!("{:?}", d.source)),
        conflicts: decision.map_or_else(Vec::new, |d| {
            d.conflicts
                .iter()
                .map(|c| format!("{:?}", c.kind))
                .collect()
        }),
    }
}

// 读取注入 tsv；读失败 warn + 空表（verify 诊断优先，不因辅助输入阻断主流程）。
// 错误仅进 report.errors（soft-cap 保护）不返 Err，让「tsv 缺失」与「tsv 为空」语义
// 都落在对账结果上而非 CLI 失败。
fn load_tsv(exif_tsv: Option<&Path>) -> Vec<ExifRow> {
    let Some(path) = exif_tsv else {
        return Vec::new();
    };
    match verify_io::read_to_string(path, crate::usecases::config::config().verify.max_tsv_bytes) {
        Ok(content) => parse_tsv(&content),
        Err(e) => {
            warn!(
                feature = FEATURE_VERIFY,
                operation = "load_exif_tsv",
                result = "error",
                tsv_path = %path.display(),
                error = %e,
                "cannot read exif tsv; cross-check disabled"
            );
            Vec::new()
        }
    }
}

fn index_tsv(rows: &[ExifRow]) -> HashMap<&str, &ExifRow> {
    let mut map = HashMap::with_capacity(rows.len());
    for r in rows {
        map.entry(r.path.as_str()).or_insert(r);
    }
    map
}

fn lookup_tsv<'a>(map: &HashMap<&str, &'a ExifRow>, path: &str) -> Option<&'a ExifRow> {
    map.get(normalize_sep(path).as_str()).copied()
}

// 剥 source 根前缀得相对路径（skill 的 SOURCE_ROOT 语义），避免根目录年份段污染
// 文件名/路径日期解析；剥不掉时回退 basename。
#[must_use]
pub(crate) fn strip_source_root(full: &str, roots: &[String]) -> String {
    let norm = normalize_sep(full);
    for r in roots {
        let root = normalize_sep(r).trim_end_matches('/').to_owned();
        if let Some(stripped) = norm.strip_prefix(&root) {
            let stripped = stripped.trim_start_matches('/');
            if !stripped.is_empty() {
                return stripped.to_owned();
            }
        }
    }
    match std::path::Path::new(full).file_name() {
        Some(f) => f.to_string_lossy().into_owned(),
        None => full.to_owned(),
    }
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
