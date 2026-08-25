//! move-text-shot 的 per-worker/per-source 累加器与失败记录。

use std::io;

use tracing::warn;

use crate::usecases::report::{FEATURE_MOVE_TEXT_SHOT, ReportError, extend_errors_capped};

/// per-worker/per-source 累加器：Vec + 计数;主线程 reduce 到全局 report。
/// 与 `copy/run.rs::CopyDelta` 同套路：并行 map + tree-reduce 归并。
#[derive(Default)]
pub(super) struct SourceDelta {
    pub(super) scanned: usize,
    pub(super) image_files: usize,
    pub(super) ocr_hits: usize,
    pub(super) moved: usize,
    pub(super) deduplicated: usize,
    pub(super) skipped_non_image: usize,
    pub(super) skipped_no_text: usize,
    pub(super) skipped_too_large: usize,
    pub(super) failed: usize,
    pub(super) errors: Vec<ReportError>,
}

pub(super) fn merge_delta(
    report: &mut crate::usecases::move_text_shot::report::MoveTextShotReport,
    delta: SourceDelta,
) {
    report.scanned += delta.scanned;
    report.image_files += delta.image_files;
    report.ocr_hits += delta.ocr_hits;
    report.moved += delta.moved;
    report.deduplicated += delta.deduplicated;
    report.skipped_non_image += delta.skipped_non_image;
    report.skipped_no_text += delta.skipped_no_text;
    report.skipped_too_large += delta.skipped_too_large;
    report.failed += delta.failed;
    extend_errors_capped(
        &mut report.errors,
        &mut report.errors_truncated,
        delta.errors,
        /* src_truncated = */ false,
    );
}

pub(super) fn reduce_delta(mut a: SourceDelta, b: SourceDelta) -> SourceDelta {
    a.scanned += b.scanned;
    a.image_files += b.image_files;
    a.ocr_hits += b.ocr_hits;
    a.moved += b.moved;
    a.deduplicated += b.deduplicated;
    a.skipped_non_image += b.skipped_non_image;
    a.skipped_no_text += b.skipped_no_text;
    a.skipped_too_large += b.skipped_too_large;
    a.failed += b.failed;
    a.errors.extend(b.errors);
    a
}

/// 单文件失败：warn 一条结构化日志 + 计入 `delta.errors`（受 `ERRORS_SOFT_CAP` 软上限
/// 保护;merge 阶段超上限时置 `errors_truncated=true`）。级别用 `warn!` 而非 `error!`：
/// 单文件失败 continue 不中断主流程，符合 CLAUDE.md `map_and_log` 分级哲学
/// （NotFound/AlreadyExists → debug!，其余可预期业务错误 → warn!）。
pub(super) fn record_failure(delta: &mut SourceDelta, path: String, e: &io::Error) {
    let msg = e.to_string();
    log_item_failed(&path, &msg);
    delta.errors.push(ReportError { path, message: msg });
    delta.failed += 1;
}

fn log_item_failed(path: &str, msg: &str) {
    warn!(
        feature = FEATURE_MOVE_TEXT_SHOT,
        operation = "process_entry",
        result = "error",
        source = %path,
        error = %msg,
        "move_text_shot item failed"
    );
}
