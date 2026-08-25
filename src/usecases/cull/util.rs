//! `cull::run` 低层 helper：日志、IO、MIME 嗅探、source-output 重叠保护。
//! 外置以让 `run.rs` 保 ≤ 512 行（P0 §7）。

use std::io::{self, Read};
use std::sync::Arc;

use tracing::error;

use super::report::CullReport;
use crate::entities::backend::Backend;
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::report::{FEATURE_CULL, ReportError, push_error_capped};

/// 从 `usecases::report` 单点 re-export，让 super 模块（`cull::run` / `group_writer`）
/// 直接 `use super::util::FEATURE` 无需再 import report 模块，同时保证与
/// `report_sink` 共用同一字符串防漂移。
pub(super) const FEATURE: &str = FEATURE_CULL;
const MIME_SNIFF_BYTES: usize = 256;

#[path = "util_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::{
    log_analyze_image, log_commit_group, log_cull_summary, log_identity_clusters, log_pick_best,
    log_scan_entry_ok, log_scan_source_complete, log_scan_source_start,
};

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
    push_error_capped(
        &mut report.errors,
        &mut report.errors_truncated,
        ReportError { path, message: msg },
    );
    report.failed += 1;
}
