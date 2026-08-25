//! `cull` 的 debug! 日志 helper 集合外置：release 默认不订阅 debug 级别，宏内
//! field-value 表达式（`clusters.len()` / `format!` / `if report.failed == 0`）在
//! per-instance 计 0-hit → phantom branch/region/line miss，供 ignore-regex 整文件排除。
//! 业务行为由 cull 集成测试（lib unit + `lib_tidy`）断言保证。

use tracing::debug;

use super::FEATURE;
use crate::usecases::cull::report::CullReport;

pub(crate) fn log_cull_summary(report: &CullReport, dry_run: bool) {
    debug!(
        feature = FEATURE,
        operation = "summary",
        result = if report.failed == 0 { "ok" } else { "partial" },
        scanned = report.scanned,
        grouped = report.grouped,
        best_count = report.best_count,
        culled_count = report.culled_count,
        moved = report.moved,
        dropped_blurry = report.dropped_blurry,
        failed = report.failed,
        dry_run,
        "cull summary"
    );
}

pub(crate) fn log_identity_clusters(clusters: &[Vec<usize>]) {
    debug!(
        feature = FEATURE,
        operation = "identity_cluster",
        result = "ok",
        cluster_count = clusters.len(),
        "identity clusters computed"
    );
}

pub(crate) fn log_scan_source_start(source: &str, max_image_bytes: u64) {
    debug!(
        feature = FEATURE,
        operation = "scan_source_start",
        source = %source,
        max_image_bytes,
        "cull scan started"
    );
}

pub(crate) fn log_scan_source_complete(source: &str, valid: usize, total_entries: usize) {
    debug!(
        feature = FEATURE,
        operation = "scan_source_complete",
        result = "ok",
        source = %source,
        valid,
        total_entries,
        "cull scan completed"
    );
}

pub(crate) fn log_scan_entry_ok(source: &str, size: u64, hash: u64, sharpness: f32) {
    debug!(
        feature = FEATURE,
        operation = "scan_entry",
        result = "ok",
        source = %source,
        size,
        hash = format!("{hash:016x}"),
        sharpness,
        "cull scan entry"
    );
}

pub(crate) fn log_analyze_image(source: &str, detections: usize, embedded: usize) {
    debug!(
        feature = FEATURE,
        operation = "analyze_image",
        result = "ok",
        source = %source,
        detections,
        embedded,
        "cull analyze image"
    );
}

pub(crate) fn log_pick_best(group_size: usize, best_idx: usize, best_total: f32) {
    debug!(
        feature = FEATURE,
        operation = "pick_best",
        result = "ok",
        group_size,
        best_idx,
        best_total,
        "cull pick best"
    );
}

pub(crate) fn log_commit_group(
    group_id: usize,
    best_path: &str,
    culled_count: usize,
    dry_run: bool,
) {
    debug!(
        feature = FEATURE,
        operation = "commit_group",
        result = "ok",
        group_id,
        best_path = %best_path,
        culled_count,
        dry_run,
        "cull commit group"
    );
}
