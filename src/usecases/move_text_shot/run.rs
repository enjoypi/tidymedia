//! `move-text-shot` 主流程：扫源 → size 上限 → MIME sniff → OCR → 按相对路径移到 output。
//!
//! 关键决策（与 plan 文件「核心算法」一致）：
//! - 不建 `Index`：copy/move 走 SHA-512 dedup；本 use case 仅需「有文本→搬」二元判定，
//!   幂等由 `do_move_file` 内 target 存在时的 SHA-512 比对处理（`deduplicated` 字段）
//! - 路径相对源 root：`output / strip_prefix(entry.path, source.path())`
//! - 同名冲突复用 `_1.._N`（`CopyConfig::unique_name_max_attempts`）
//! - 移动 = `Backend::supports_native_rename_to` 返 true → 走 `src.rename` fast-path；
//!   否则 `stream_copy` + remove 单点走 `entities::backend::stream_copy`（跨 scheme 或跨实例）
//! - Overlap 保护：source ⊆ output → `InvalidInput`；sources 间互相重叠 → `InvalidInput`；
//!   output ⊂ source → walk 时按 `canonical_prefix` 跳过子树（symlink 透明）
//! - Rayon 并行：source 内 walk 到 `Vec<Entry>` 后 `par_iter`，per-worker `SourceDelta`
//!   累加 reduce（同 `copy/run.rs::run_copy_loop` 分桶并行套路）；`mkdir_cache` 用
//!   `Arc<DashSet<Location>>` 跨 worker 共享 &self contains+insert 无锁
//! - 单文件字节上限 `backend.ocr.max_image_bytes` 前置 skip（防 OOM，同 cull 套路）
//!
//! 拆分为四个子模块，本文件只留入口编排与 overlap 校验：
//! - `delta`：per-worker 累加器 + 失败记录
//! - `scan`：walk + size/sniff 过滤 + OCR 判定
//! - `target`：搬移目标决策（幂等去重 + `unique_name` 分派）
//! - `move_file`：真正的 IO 搬移 + dry-run 分派

use std::io;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashSet;
use tracing::debug;

use super::delta::merge_delta;
use super::report::MoveTextShotReport;
use super::scan::process_source;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::ocr::TextDetector;
use crate::usecases::report::FEATURE_MOVE_TEXT_SHOT;

/// `FEATURE` 从 `usecases::report` 单点 import（原本地 const 已删除，与 `report_sink` /
/// 其他 use case 共用防漂移）。
const FEATURE: &str = FEATURE_MOVE_TEXT_SHOT;

#[cfg(test)]
use super::delta::{SourceDelta, reduce_delta};
#[cfg(test)]
use super::move_file::target_dir;
#[cfg(test)]
use super::scan::{is_entry_under_output, is_image};
#[cfg(test)]
use super::target::{relative_to, split_stem_ext, unique_name_from_index};
#[cfg(test)]
use crate::entities::backend::Backend;
#[cfg(test)]
use crate::usecases::report::ReportError;
#[cfg(test)]
use camino::Utf8Path;

/// 入口：列扫 sources，把含文本的 image 文件按相对路径搬到 output。
///
/// # Errors
///
/// - source ⊆ output 或 sources 间互相重叠：返 `InvalidInput`（重叠保护）
/// - factory 构造 backend 失败传播
/// - 单文件失败不中断主流程，累计到 `report.failed`/`errors`
pub fn move_text_shot(
    detector: &dyn TextDetector,
    factory: &dyn BackendFactory,
    sources: &[Location],
    output: &Location,
    dry_run: bool,
) -> common::Result<MoveTextShotReport> {
    let start = Instant::now();
    let output_backend = factory.for_location(output)?;
    let output_prefix = canonical_prefix(output);

    ensure_no_overlap(sources, &output_prefix)?;

    let mkdir_cache: Arc<DashSet<Location>> = Arc::new(DashSet::new());
    let max_image_bytes = config().backend.ocr.max_image_bytes;

    let mut report = MoveTextShotReport {
        dry_run,
        ..MoveTextShotReport::default()
    };

    // source 数量通常 ≤ 10 → 逐 source 串行；source 内的 entries 用 rayon 并行
    // （has_text 是 tract CPU 密集推理，rayon worker 独立解码+推理让多核充分利用）。
    for source in sources {
        let src_backend = factory.for_location(source)?;
        let delta = process_source(
            detector,
            source,
            &src_backend,
            output,
            &output_backend,
            &output_prefix,
            &mkdir_cache,
            max_image_bytes,
            dry_run,
        );
        merge_delta(&mut report, delta);
    }

    report.duration_ms = crate::usecases::report::elapsed_ms(start);
    log_summary(&report, dry_run);
    Ok(report)
}

fn log_summary(report: &MoveTextShotReport, dry_run: bool) {
    let result = summary_result(report.failed);
    debug!(
        feature = FEATURE,
        operation = "summary",
        result,
        scanned = report.scanned,
        image_files = report.image_files,
        ocr_hits = report.ocr_hits,
        moved = report.moved,
        deduplicated = report.deduplicated,
        skipped_non_image = report.skipped_non_image,
        skipped_no_text = report.skipped_no_text,
        skipped_too_large = report.skipped_too_large,
        failed = report.failed,
        errors_truncated = report.errors_truncated,
        dry_run,
        "move_text_shot summary"
    );
}

pub(super) fn summary_result(failed: usize) -> &'static str {
    if failed == 0 { "ok" } else { "partial" }
}

/// 校验 sources ⊄ output && sources 之间不互相包含。任一违反即返 `InvalidInput`，
/// 由 CLI 层退出码非 0 让用户重新组织路径参数；夹带一致提示文案让 tidy-verify 类脚本
/// 好识别（`is inside output` / `overlaps another source`）。
fn ensure_no_overlap(sources: &[Location], output_prefix: &str) -> common::Result<()> {
    // 1) sources 与 output 重叠
    for src in sources {
        let prefix = canonical_prefix(src);
        if under_prefix(&prefix, output_prefix) {
            return Err(common::Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "source {prefix} is inside output {output_prefix}; \
                     move would archive files into themselves"
                ),
            )));
        }
    }
    // 2) sources 两两之间重叠 → walker 会重复枚举同一文件让第二次 rename 报「源已消失」
    //    伪失败；提前挡在入口。O(n²) 对 sources 一般 ≤ 10 忽略不计。
    let canonicals: Vec<String> = sources.iter().map(canonical_prefix).collect();
    for i in 0..canonicals.len() {
        for j in 0..canonicals.len() {
            if i == j {
                continue;
            }
            if under_prefix(&canonicals[i], &canonicals[j]) {
                return Err(common::Error::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "source {a} overlaps another source {b}; \
                         each file must be reachable from at most one source root",
                        a = canonicals[i],
                        b = canonicals[j],
                    ),
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
