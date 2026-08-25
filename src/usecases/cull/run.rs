//! `cull` 主流程：scan → pHash 分组 → 4 模型印证评分 → `group_writer` 落盘。
//!
//! 5 阶段：
//! 1. **扫源**：walk 所有 source，按 `max_image_bytes` 跳过超大文件；逐张读字节 +
//!    `image::load_from_memory` 解码 + 计算 pHash + 灰度清晰度，**计算完即 drop 字节与
//!    解码图**——仅 `ScannedFile`（metadata + hash + sharpness）入 `Vec` 累计。
//!    旧实现把 `Arc<Vec<u8>>` + `Arc<RgbImage>` 也存进 `ScannedFile` 让 `pick_best` 阶段复用，
//!    但 `scan` 主循环会让整批图（2847 张 × ~17 MB ≈ 48 GB）驻留致 OOM；现按需在
//!    `analyze_image` 内对组内成员重读+重 decode（仅多图组承担二次开销，单图组直接跳过）。
//! 2. **pHash 分组**：Union-Find 按汉明距离 ≤ `phash_hamming_max` 分组
//! 3. **粗筛**：单图组（len=1）跳过：无可比较对象不搬迁不入 report
//! 4. **评分**：每组 per-image 跑 SCRFD → 5 点对齐 → `MobileFaceNet` 128 维 embedding +
//!    `FaceMesh` 468 点 EAR + `EyeState` 闭眼概率，`face_scoring::score_image` 出
//!    `ScoreBreakdown`；组内全部图算完后调 `identity_cluster::cluster_identities` 输出
//!    跨图身份簇日志；选 `breakdown.total` 最高者为 best
//! 5. **落盘**：调 `group_writer::write_group` 写 group 目录
//!
//! 拆分为三个子模块，本文件只留入口编排：
//! - `scan`：扫源（walk + 过滤 + read/decode + phash/sharpness）
//! - `score`：组级 4 模型印证评分 + 选 best
//! - `commit`：过滤单图/模糊组 + 落盘 group
use std::io;
use std::time::Instant;

use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::commit::{commit_scored_group, filter_multi_image_groups};
use super::phash::group_by_hash;
use super::report::{CullReport, ScoreBreakdown};
use super::scan::{ScannedFile, scan_source};
use super::score::pick_best_for_group;
use super::util::{ensure_sources_outside_output, log_cull_summary, record_failure};
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{self, canonical_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::face::{EyeStateClassifier, FaceDetector, FaceEmbedder, FaceMeshDetector};

/// 并行 ONNX 评分后由主线程串行写盘的中间态：保 indices 与 breakdowns 顺序对齐
/// `pick_best_for_group` 输出。
pub(super) struct ScoredGroup {
    pub(super) indices: Vec<usize>,
    pub(super) best_idx: usize,
    pub(super) best_breakdown: ScoreBreakdown,
    pub(super) breakdowns: Vec<ScoreBreakdown>,
    pub(super) failures: Vec<(String, io::Error)>,
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
    let start = Instant::now();
    let face_cfg = &config().backend.face;
    let output_backend = factory.for_location(output)?;
    // canonical_prefix 让 symlink output（如 /tmp/out → /photos/cull_output）下
    // src 路径与 output prefix 字面可比；裸 display() 会让 under_prefix 误返 false
    // 致 move 模式把 output 内文件再次搬迁自身。
    let output_prefix = canonical_prefix(output);
    ensure_sources_outside_output(sources, &output_prefix)?;

    let mut report = CullReport {
        dry_run,
        ..CullReport::default()
    };
    if !dry_run {
        output_backend.mkdir_p(output)?;
    }

    let mut scanned: Vec<ScannedFile> = Vec::new();
    for source in sources {
        let src_backend = factory.for_location(source)?;
        scan_source(
            source,
            &src_backend,
            &output_prefix,
            face_cfg,
            &mut scanned,
            &mut report,
        );
    }
    // report.scanned 由 scan_source 增量累加触达的 source 文件数（含被识别为非媒体
    // 跳过、超大跳过、解码失败、IO 失败），口径与 CopyReport.scanned 一致；
    // 而 scanned vec 仅含成功解码的图（用于后续分组/评分），不是 report 的 scanned。

    let hashes: Vec<u64> = scanned.iter().map(|s| s.hash).collect();
    let groups = group_by_hash(&hashes, phash_max_hamming);

    // 阶段 A：模糊过滤串行（写 report.dropped_blurry）+ 收集合规多图组。
    let (filtered, blurry_dropped) =
        filter_multi_image_groups(groups, &scanned, face_cfg.sharpness_min);
    report.dropped_blurry += blurry_dropped;

    // 阶段 B：组级 ONNX 评分并行（4 个 face trait 都是 Send+Sync，
    // tract `TypedRunnableModel::run(&self,...)` 支持 `&self` 并发调用）。
    let scored: Vec<ScoredGroup> = filtered
        .into_par_iter()
        .map(|indices| {
            let mut failures = Vec::new();
            let (best_idx, best_breakdown, breakdowns) = pick_best_for_group(
                &indices,
                &scanned,
                scrfd,
                facenet,
                facemesh,
                eyestate,
                face_cfg,
                &mut failures,
            );
            ScoredGroup {
                indices,
                best_idx,
                best_breakdown,
                breakdowns,
                failures,
            }
        })
        .collect();

    // 阶段 C：写盘串行（write_group 内含 mkdir_p / copy_file / unique_name 等 IO，
    // 串行让 group_id 单调分配 + report 顺序合并 + 远端 backend 不需并发安全契约）。
    let mut moved = 0_usize;
    let mut next_group_id = 1_usize;
    for mut sg in scored {
        // failures 先排走避免 commit_scored_group 借用 sg 时 partial move。
        let failures = std::mem::take(&mut sg.failures);
        for (path, err) in failures {
            record_failure(&mut report, path, &err);
        }
        commit_scored_group(
            sg,
            &scanned,
            output,
            &output_backend,
            dry_run,
            &mut next_group_id,
            &mut moved,
            &mut report,
        );
    }
    report.moved = moved;

    report.duration_ms = crate::usecases::report::elapsed_ms(start);
    log_cull_summary(&report, dry_run);
    Ok(report)
}

#[cfg(test)]
use super::commit::filter_blurry;
#[cfg(test)]
use super::crop::{
    crop_eye_around, crop_face_bbox, total_cmp_nan_as_neg_inf, u32_from_f32_clamped,
};
#[cfg(test)]
use super::score::analyze_image;

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
