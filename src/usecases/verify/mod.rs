//! verify use case：把「tidy-verify skill」的确定性对账业务内化为子命令。
//! 阶段化落地：骨架（本文件）→ 桶对账 `bucket.rs`/`exif_tsv.rs` → 文件名/目录段
//! 提示 `filename_hint.rs` → 内容像素比对 `content_diff.rs` → 9 pattern 诊断
//! `diagnose.rs`。

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::debug;
use tracing::warn;

use crate::entities::file_index;
use crate::entities::file_info::Info;
use crate::entities::media_time::MediaTimeDecision;
use crate::usecases::report::elapsed_ms;

use super::copy::Source;
use super::report::FEATURE_VERIFY;
use super::verify::content_diff::{OutputLibrary, build_output_index};
use super::verify::evaluate::{EvalCtx, Evaluated, evaluate};
use super::verify::exif_tsv::{ExifRow, normalize_sep, parse_tsv};
pub use report::{VerifyEntry, VerifyReport};
pub(crate) use summary::format_summary;

pub(crate) mod bucket;
pub(crate) mod candidates;
pub(crate) mod content_diff;
pub(crate) mod diagnose;
mod evaluate;
pub(crate) mod exif_tsv;
pub(crate) mod filename_hint;
pub(crate) mod report;
pub(crate) mod summary;

// verify 的 pHash 召回阈值：与 cull 分组的 `backend.face.phash_hamming_max`(10)
// 不同调——image crate JPEG 重编码（decode→re-encode）实测 hamming 16~18，阈值
// 10 让旋转/重编码同媒体漏召回到 name_only；宽召回的假阳性由 L4 逐像素均差
// 复核（mean_abs_diff < 5）拦截，见 content_diff::rotated_phash_similar。
pub(crate) const DEFAULT_PHASH_MAX: u8 = 20;

/// 归档验证：扫描源端，对每个文件做拍摄时间决策与桶对账；注入 `--exif-tsv`
/// 时用第二实现（exiftool）的期望桶交叉比对。只诊断不写盘。
pub(crate) fn verify(
    sources: &[Source],
    output: &Source,
    phash_max: u8,
    include_non_media: bool,
    exif_tsv: Option<&Path>,
) -> VerifyReport {
    let start = Instant::now();
    let skip_ext = super::copy::source_skip_extensions(include_non_media, false);
    let mut src_index = file_index::Index::new();
    for (loc, backend) in sources {
        src_index.visit_location_skipping(loc, backend, skip_ext);
    }
    let tz = crate::usecases::config::config().copy.timezone_offset_hours;
    let offset = crate::usecases::config::chrono_offset_from_hours(tz);
    src_index.parse_exif(offset, include_non_media, false);
    let scanned = src_index.len();
    debug!(
        feature = FEATURE_VERIFY,
        operation = "scan_complete",
        result = "ok",
        files = scanned,
        "verify source index built"
    );
    let tsv_rows = load_tsv(exif_tsv);
    let tsv = index_tsv(&tsv_rows);
    let roots: Vec<String> = sources.iter().map(|(loc, _)| loc.display()).collect();
    let out = OutputLibrary::new(build_output_index(output));
    let ctx = EvalCtx {
        offset,
        tz,
        tsv: &tsv,
        roots: &roots,
        out: &out,
        phash_max,
        max_bytes: crate::usecases::config::config()
            .backend
            .face
            .max_image_bytes,
    };
    let infos: Vec<Info> = src_index
        .into_infos()
        .into_iter()
        .filter(|i| include_non_media || i.is_media())
        .collect();
    let evaluated: Vec<Evaluated> = infos.par_iter().map(|i| evaluate(i, &ctx)).collect();
    let mut report = VerifyReport {
        scanned,
        include_non_media,
        dry_run: true,
        ..Default::default()
    };
    fold_evaluated(&mut report, evaluated);
    report.duration_ms = elapsed_ms(start);
    report
}

fn fold_evaluated(report: &mut VerifyReport, mut evaluated: Vec<Evaluated>) {
    evaluated.sort_by(|a, b| a.entry.source_path.cmp(&b.entry.source_path));
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
        report.entries.push(ev.entry);
    }
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
    match std::fs::read_to_string(path) {
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
#[doc(hidden)]
#[must_use]
pub fn strip_source_root(full: &str, roots: &[String]) -> String {
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
