//! verify use case：把「tidy-verify skill」的确定性对账业务内化为子命令。
//! 阶段化落地：骨架（本文件）→ 桶对账 `bucket.rs`/`exif_tsv.rs` → 文件名/目录段
//! 提示 `filename_hint.rs` → 内容像素比对 `content_diff.rs` → 9 pattern 诊断
//! `diagnose.rs`。

use std::path::Path;
use std::time::Instant;

use tracing::debug;
use tracing::warn;

use crate::entities::file_index;
use crate::entities::media_time::ConflictKind;
use crate::entities::media_time::MediaTimeDecision;
use crate::usecases::report::elapsed_ms;

use super::copy::Source;
use super::report::FEATURE_VERIFY;
use super::verify::bucket::actual_bucket;
use super::verify::diagnose::{DiagnoseInput, fix_suggestion, patterns};
use super::verify::exif_tsv::{ExifRow, expected_bucket, normalize_sep, parse_tsv};
use super::verify::filename_hint::parse_path_date_bucket;
pub use report::{VerifyEntry, VerifyReport};
pub(crate) use summary::format_summary;

pub(crate) mod bucket;
pub(crate) mod content_diff;
pub(crate) mod diagnose;
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
    let mut src_index = file_index::Index::new();
    for (loc, backend) in sources {
        src_index.visit_location(loc, backend);
    }
    let tz = crate::usecases::config::config().copy.timezone_offset_hours;
    let offset = crate::usecases::config::chrono_offset_from_hours(tz);
    // 与 copy 同口径：parse_exif 填 MIME 才能判 is_media / 走 P0..P1 候选；
    // include_non_media=false 时对非媒体 MIME 短路解析。
    src_index.parse_exif(offset, include_non_media, false);
    debug!(
        feature = FEATURE_VERIFY,
        operation = "scan_complete",
        result = "ok",
        files = src_index.len(),
        "verify source index built"
    );

    let tsv_rows = load_tsv(exif_tsv);
    let roots: Vec<String> = sources.iter().map(|(loc, _)| loc.display()).collect();
    let out_index = super::verify::content_diff::build_output_index(output);
    let max_bytes = crate::usecases::config::config()
        .backend
        .face
        .max_image_bytes;
    let mut report = VerifyReport {
        scanned: src_index.len(),
        include_non_media,
        dry_run: true,
        ..Default::default()
    };
    for item in src_index.iter() {
        let info = item.value();
        // 与 copy 落盘过滤同口径：非媒体文件不入对账（skill 只对 copy_file 行比对）。
        if !include_non_media && !info.is_media() {
            continue;
        }
        report.compared += 1;
        let decision = info.media_time_decision(offset);
        if decision.is_none() {
            report.decision_failed += 1;
        }
        let bucket = decision.as_ref().map(|d| actual_bucket(d.utc, offset));
        let exif = lookup_tsv(&tsv_rows, info.full_path.as_str());
        let (exp, from_label) = exif.map_or((None, None), |r| expected_bucket(r, tz));
        let mismatch = match (exp.as_deref(), bucket.as_deref()) {
            (Some(e), Some(b)) => e != b,
            _ => false,
        };
        if mismatch {
            report.mismatched += 1;
        }
        let filename_bucket =
            parse_path_date_bucket(&strip_source_root(info.full_path.as_str(), &roots));
        let verdict =
            super::verify::content_diff::verdict_for(info, &out_index, phash_max, max_bytes);
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
        let pats = patterns(&diag);
        for p in &pats {
            *report.pattern_counts.entry(p.clone()).or_insert(0) += 1;
        }
        let suggestion = fix_suggestion(&diag);
        report.entries.push(build_entry(
            info.full_path.as_str(),
            decision.as_ref(),
            bucket,
            exp,
            mismatch,
            from_label,
            exif.map(|r| r.make.clone()),
            exif.map(|r| r.model.clone()),
            filename_bucket,
            verdict,
            pats,
            suggestion,
        ));
    }
    report.duration_ms = elapsed_ms(start);
    report
}

#[allow(clippy::too_many_arguments)]
fn build_entry(
    path: &str,
    decision: Option<&MediaTimeDecision>,
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
) -> VerifyEntry {
    VerifyEntry {
        source_path: path.to_string(),
        actual_bucket: actual_bucket.unwrap_or_default(),
        exif_exp_bucket,
        exif_from,
        exif_make,
        exif_model,
        filename_bucket,
        duplicate_verdict,
        patterns,
        fix_suggestion,
        mismatch,
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

fn lookup_tsv<'a>(rows: &'a [ExifRow], path: &str) -> Option<&'a ExifRow> {
    let norm = normalize_sep(path);
    rows.iter().find(|r| r.path == norm)
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
