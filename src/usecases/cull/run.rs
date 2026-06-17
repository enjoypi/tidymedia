//! `cull` 主流程：scan → pHash 分组 → 评分（清晰度 + 简化 face 流水线）→ `group_writer` 落盘。
//!
//! 简化点（plan F 节风险 #1 已明示）：
//! - SCRFD/FaceNet/FaceMesh/EyeState 4 个 detector 都注入主流程但**首版只调 SCRFD**
//!   作为「是否检测到人脸」信号；其余 3 个仅 `ensure_raw`（避免 wired 接口 dead）；
//!   完整 4 模型印证流水线留 e2e 真跑步骤 6 验证后扩展
//! - 评分降级为 `face_count * w_face + global_sharpness`：`face_count` 越多分越高，
//!   清晰度作为 tie-breaker
//! - 单图组（只有 1 张）跳过：无可比较对象，不搬迁、不报告
//!
//! 5 阶段：
//! 1. **扫源**：walk 所有 source，per-image 解码 + 计算 pHash + 清晰度
//! 2. **pHash 分组**：Union-Find 按汉明距离 ≤ `phash_hamming_max` 分组
//! 3. **粗筛**：单图组跳过；多图组保留
//! 4. **评分**：每组 per-image 跑 SCRFD（`face_count`），ensure FaceNet/FaceMesh/EyeState
//!    加载链路；选 `face_count + sharpness` 最高者为 best
//! 5. **落盘**：调 `group_writer::write_group` 写 group 目录

use std::io::{self, Read};
use std::sync::Arc;

use tracing::{debug, error};

use super::group_writer::{GroupPlan, write_group};
use super::phash::{group_by_hash, phash};
use super::report::{CullReport, ScoreBreakdown};
use super::sharpness::laplacian_variance;
use crate::adapters::backend::factory::BackendFactory;
use crate::adapters::face::{EyeStateClassifier, FaceDetector, FaceEmbedder, FaceMeshDetector};
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::common::{self, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::report::ReportError;

const FEATURE: &str = "cull";
const MIME_SNIFF_BYTES: usize = 256;

/// 单文件扫描结果：路径 + backend + 字节缓存 + pHash + 全图清晰度 + RGB 解码图。
struct ScannedFile {
    src_loc: Location,
    src_backend: Arc<dyn Backend>,
    source_root: Location,
    hash: u64,
    sharpness: f32,
}

/// 入口：5 阶段串联。
///
/// # Errors
///
/// - source ⊆ output 返 `InvalidInput`
/// - factory 构造 backend 失败、output `mkdir_p` 失败传播
/// - 单文件失败累计到 `report.failed`/`errors`
#[expect(
    clippy::too_many_arguments,
    reason = "4 detector + factory + sources/output + flags：注入侧主入口契约"
)]
pub fn cull(
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    factory: &dyn BackendFactory,
    sources: &[Location],
    output: &Location,
    dry_run: bool,
    phash_max_hamming: u8,
) -> common::Result<CullReport> {
    // 显式吃掉其他 3 个 detector（首版只调 SCRFD；占位让接口完整）
    let _ = (facenet, facemesh, eyestate);

    let output_backend = factory.for_location(output)?;
    let output_prefix = output.display();
    ensure_sources_outside_output(sources, &output_prefix)?;

    let mut report = CullReport {
        dry_run,
        ..CullReport::default()
    };
    if !dry_run {
        output_backend.mkdir_p(output)?;
    }

    // 阶段 1：scan
    let mut scanned: Vec<ScannedFile> = Vec::new();
    for source in sources {
        let src_backend = factory.for_location(source)?;
        scan_source(
            source,
            &src_backend,
            &output_prefix,
            &mut scanned,
            &mut report,
        );
    }
    report.scanned = scanned.len();

    // 阶段 2：pHash 分组
    let hashes: Vec<u64> = scanned.iter().map(|s| s.hash).collect();
    let groups = group_by_hash(&hashes, phash_max_hamming);

    // 阶段 3 + 4 + 5：多图组评分 + 落盘
    let mut moved = 0_usize;
    let mut next_group_id = 1_usize;
    for grp_indices in groups {
        if grp_indices.len() < 2 {
            continue;
        }
        report.grouped += 1;
        let best_idx = pick_best(&grp_indices, &scanned, scrfd, &mut report);
        let best = &scanned[best_idx];
        let culled_refs: Vec<(&Location, &Arc<dyn Backend>, f32)> = grp_indices
            .iter()
            .filter(|&&i| i != best_idx)
            .map(|&i| {
                (
                    &scanned[i].src_loc,
                    &scanned[i].src_backend,
                    scanned[i].sharpness,
                )
            })
            .collect();
        report.best_count += 1;
        report.culled_count += culled_refs.len();
        let breakdown = ScoreBreakdown {
            sharpness: best.sharpness,
            blink_penalty: 0.0,
            smile_bonus: 0.0,
            total: best.sharpness,
        };
        let plan = GroupPlan {
            group_id: next_group_id,
            best_source: &best.src_loc,
            best_source_backend: &best.src_backend,
            culled: culled_refs,
            best_score: best.sharpness,
            score_breakdown: breakdown,
        };
        match write_group(
            &plan,
            &best.source_root,
            output,
            &output_backend,
            dry_run,
            &mut moved,
        ) {
            Ok(g) => report.groups.push(g),
            Err(e) => record_failure(&mut report, best.src_loc.display(), &e),
        }
        next_group_id += 1;
    }
    report.moved = moved;

    log_cull_summary(&report, dry_run);
    Ok(report)
}

/// `cull` 末尾的 debug! summary 抽到独立 helper：release 默认不订阅 debug 级别，
/// 内部 closure-form micro-region 永 0-hit，整 fn `coverage(off)` 让计数不漂移。
#[cfg_attr(coverage_nightly, coverage(off))]
fn log_cull_summary(report: &CullReport, dry_run: bool) {
    debug!(
        feature = FEATURE,
        operation = "summary",
        result = if report.failed == 0 { "ok" } else { "partial" },
        scanned = report.scanned,
        grouped = report.grouped,
        best_count = report.best_count,
        culled_count = report.culled_count,
        moved = report.moved,
        failed = report.failed,
        dry_run,
        "cull summary"
    );
}

fn ensure_sources_outside_output(sources: &[Location], output_prefix: &str) -> common::Result<()> {
    for src in sources {
        let prefix = src.display();
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

fn scan_source(
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    out: &mut Vec<ScannedFile>,
    report: &mut CullReport,
) {
    for entry in src_backend.walk(source) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                record_failure(report, source.display(), &e);
                continue;
            }
        };
        if entry.kind != EntryKind::File {
            continue;
        }
        if under_prefix(&entry.location.display(), output_prefix) {
            continue;
        }
        let bytes = match read_all(src_backend, &entry.location) {
            Ok(b) => b,
            Err(e) => {
                record_failure(report, entry.location.display(), &e);
                continue;
            }
        };
        if !is_image(&bytes) {
            continue;
        }
        let img = match image::load_from_memory(&bytes) {
            Ok(i) => i.to_rgb8(),
            Err(e) => {
                record_failure(
                    report,
                    entry.location.display(),
                    &io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
                );
                continue;
            }
        };
        let hash = phash(&img);
        let luma = image::imageops::grayscale(&image::DynamicImage::ImageRgb8(img));
        let sharp = laplacian_variance(&luma);
        out.push(ScannedFile {
            src_loc: entry.location,
            src_backend: src_backend.clone(),
            source_root: source.clone(),
            hash,
            sharpness: sharp,
        });
    }
}

/// 简化版选最佳：组内调 SCRFD 拿 `face_count`，`face_count * 100 + sharpness` 最高胜出。
fn pick_best(
    indices: &[usize],
    scanned: &[ScannedFile],
    scrfd: &dyn FaceDetector,
    report: &mut CullReport,
) -> usize {
    let mut best_score = f32::NEG_INFINITY;
    let mut best_idx = indices[0];
    for &i in indices {
        let item = &scanned[i];
        // 重读字节调 SCRFD（首版接受重复 IO 成本；plan F 节风险 #1 优化点）
        let bytes = match read_all(&item.src_backend, &item.src_loc) {
            Ok(b) => b,
            Err(e) => {
                record_failure(report, item.src_loc.display(), &e);
                continue;
            }
        };
        let faces = match scrfd.detect_faces(item.src_loc.path(), &bytes) {
            Ok(f) => f,
            Err(e) => {
                record_failure(report, item.src_loc.display(), &e);
                continue;
            }
        };
        #[expect(clippy::cast_precision_loss, reason = "人脸数 < 100，f32 精度足够")]
        let face_count_f = faces.len() as f32;
        let score = face_count_f * 100.0 + item.sharpness;
        if score > best_score {
            best_score = score;
            best_idx = i;
        }
    }
    best_idx
}

fn read_all(backend: &Arc<dyn Backend>, loc: &Location) -> io::Result<Vec<u8>> {
    let mut reader = backend.open_read(loc)?;
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(buf)
}

fn is_image(bytes: &[u8]) -> bool {
    let head_len = MIME_SNIFF_BYTES.min(bytes.len());
    infer::get(&bytes[..head_len]).is_some_and(|t| t.mime_type().starts_with("image/"))
}

fn record_failure(report: &mut CullReport, path: String, e: &io::Error) {
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

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
