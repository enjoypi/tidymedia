//! `cull::run` 低层 helper：日志、IO、MIME 嗅探、source-output 重叠保护。
//! 外置以让 `run.rs` 保 ≤ 512 行（P0 §7）。

use std::io::{self, Read};
use std::sync::Arc;

use tracing::{debug, error};

use super::report::CullReport;
use crate::entities::backend::Backend;
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::report::ReportError;

pub(super) const FEATURE: &str = "cull";
const MIME_SNIFF_BYTES: usize = 256;

/// `cull` 末尾的 debug! summary 抽独立 helper：release 默认不订阅 debug 级别，
/// 内部 closure-form micro-region 永 0-hit，整 fn `coverage(off)` 让计数不漂移。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn log_cull_summary(report: &CullReport, dry_run: bool) {
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

/// 同 `log_cull_summary` 套路：身份簇 debug! 输出抽独立 fn，release 不订阅 → 0-hit。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn log_identity_clusters(clusters: &[Vec<usize>]) {
    debug!(
        feature = FEATURE,
        operation = "identity_cluster",
        result = "ok",
        cluster_count = clusters.len(),
        "identity clusters computed"
    );
}

/// source ⊆ output 重叠保护：避免 cull 把文件归档到自身路径下导致循环搬迁。
pub(super) fn ensure_sources_outside_output(
    sources: &[Location],
    output_prefix: &str,
) -> common::Result<()> {
    for src in sources {
        let prefix = canonical_prefix(src);
        if under_prefix(&prefix, output_prefix) {
            return Err(common::Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "source {prefix} is inside output {output_prefix}; \
                     cull would archive files into themselves"
                ),
            )));
        }
    }
    Ok(())
}

pub(super) fn read_all(backend: &Arc<dyn Backend>, loc: &Location) -> io::Result<Vec<u8>> {
    let mut reader = backend.open_read(loc)?;
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(buf)
}

pub(super) fn is_image(bytes: &[u8]) -> bool {
    let head_len = MIME_SNIFF_BYTES.min(bytes.len());
    infer::get(&bytes[..head_len]).is_some_and(|t| t.mime_type().starts_with("image/"))
}

pub(super) fn record_failure(report: &mut CullReport, path: String, e: &io::Error) {
    let msg = e.to_string();
    error!(
        feature = FEATURE,
        operation = "process_entry",
        result = "error",
        source = %path,
        error = %msg,
        "cull item failed"
    );
    report.errors.push(ReportError { path, message: msg });
    report.failed += 1;
}
