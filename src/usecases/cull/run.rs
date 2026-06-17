//! `cull` 主流程：scan → pHash 分组 → 4 模型印证评分 → `group_writer` 落盘。
//!
//! 5 阶段：
//! 1. **扫源**：walk 所有 source，按 `max_image_bytes` 跳过超大文件；剩下一次读字节 +
//!    `image::load_from_memory` 解码 + 计算 pHash + 灰度清晰度。字节与 RGB 解码图都装
//!    `Arc` 缓存，供 `pick_best` 复用（消除二次 IO 与二次 decode）
//! 2. **pHash 分组**：Union-Find 按汉明距离 ≤ `phash_hamming_max` 分组
//! 3. **粗筛**：单图组（len=1）跳过：无可比较对象不搬迁不入 report
//! 4. **评分**：每组 per-image 跑 SCRFD → 5 点对齐 → `MobileFaceNet` 128 维 embedding +
//!    `FaceMesh` 468 点 EAR + `EyeState` 闭眼概率，`face_scoring::score_image` 出
//!    `ScoreBreakdown`；组内全部图算完后调 `identity_cluster::cluster_identities` 输出
//!    跨图身份簇日志；选 `breakdown.total` 最高者为 best
//! 5. **落盘**：调 `group_writer::write_group` 写 group 目录

use std::io::{self, Read};
use std::sync::Arc;

use image::{DynamicImage, RgbImage};
use tracing::{debug, error};

use super::face_align;
use super::face_scoring;
use super::group_writer::{GroupPlan, write_group};
use super::identity_cluster;
use super::phash::{group_by_hash, phash};
use super::report::{CullReport, ScoreBreakdown};
use super::sharpness::laplacian_variance;
use crate::adapters::backend::factory::BackendFactory;
use crate::adapters::face::{
    EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector,
};
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::{FaceConfig, config};
use crate::usecases::report::ReportError;

const FEATURE: &str = "cull";
const MIME_SNIFF_BYTES: usize = 256;

/// 眼部 crop 半径相对 face bbox 高度的比例（左右眼各 crop 一次给 `EyeState` 模型）。
const EYE_CROP_RADIUS_RATIO: f32 = 0.10;

/// 单文件扫描结果。`raw_bytes`/`decoded` 装 `Arc` 让 `pick_best` 阶段复用，省二次 IO 与 decode。
struct ScannedFile {
    src_loc: Location,
    src_backend: Arc<dyn Backend>,
    source_root: Location,
    hash: u64,
    sharpness: f32,
    raw_bytes: Arc<Vec<u8>>,
    decoded: Arc<RgbImage>,
}

/// 单张图 4 模型印证结果（faces 长度与其余 3 vec 一致：对齐失败/嵌入失败的 face 整体丢弃）。
struct ImageAnalysis {
    faces: Vec<FaceDetection>,
    embeddings: Vec<[f32; identity_cluster::EMBED_DIM]>,
    meshes: Vec<Vec<[f32; 3]>>,
    eye_states: Vec<(f32, f32)>,
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
    report.scanned = scanned.len();

    let hashes: Vec<u64> = scanned.iter().map(|s| s.hash).collect();
    let groups = group_by_hash(&hashes, phash_max_hamming);

    let mut moved = 0_usize;
    let mut next_group_id = 1_usize;
    for grp_indices in groups {
        if grp_indices.len() < 2 {
            continue;
        }
        process_group(
            &grp_indices,
            &scanned,
            scrfd,
            facenet,
            facemesh,
            eyestate,
            face_cfg,
            output,
            &output_backend,
            dry_run,
            &mut next_group_id,
            &mut moved,
            &mut report,
        );
    }
    report.moved = moved;

    log_cull_summary(&report, dry_run);
    Ok(report)
}

/// 处理单个相似组：评分选 best + 落盘。封装让 `cull` 主体保持简洁。
#[expect(
    clippy::too_many_arguments,
    reason = "组处理需要 4 detector + cfg + output backend + 计数器：内聚一处"
)]
fn process_group(
    grp_indices: &[usize],
    scanned: &[ScannedFile],
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    dry_run: bool,
    next_group_id: &mut usize,
    moved: &mut usize,
    report: &mut CullReport,
) {
    report.grouped += 1;
    let (best_idx, best_breakdown) = pick_best_for_group(
        grp_indices,
        scanned,
        scrfd,
        facenet,
        facemesh,
        eyestate,
        face_cfg,
        report,
    );
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
    let culled_len = culled_refs.len();
    let plan = GroupPlan {
        group_id: *next_group_id,
        best_source: &best.src_loc,
        best_source_backend: &best.src_backend,
        culled: culled_refs,
        best_score: best_breakdown.total,
        score_breakdown: best_breakdown,
    };
    // 计数搬到 Ok arm 内：write_group Err 时 groups 不 push，best_count/culled_count
    // 也必须保持原子（曾经在外提前累加，让 best_count != groups.len() 误导消费方）。
    match write_group(
        &plan,
        &best.source_root,
        output,
        output_backend,
        dry_run,
        moved,
    ) {
        Ok(g) => {
            report.best_count += 1;
            report.culled_count += culled_len;
            report.groups.push(g);
        }
        Err(e) => record_failure(report, best.src_loc.display(), &e),
    }
    *next_group_id += 1;
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

/// 同 `log_cull_summary` 套路：身份簇 debug! 输出抽独立 fn，release 不订阅 → 0-hit。
#[cfg_attr(coverage_nightly, coverage(off))]
fn log_identity_clusters(clusters: &[Vec<usize>]) {
    debug!(
        feature = FEATURE,
        operation = "identity_cluster",
        result = "ok",
        cluster_count = clusters.len(),
        "identity clusters computed"
    );
}

fn ensure_sources_outside_output(sources: &[Location], output_prefix: &str) -> common::Result<()> {
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

fn scan_source(
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    face_cfg: &FaceConfig,
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
        if entry.size > face_cfg.max_image_bytes {
            let err = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cull: file {bytes} bytes exceeds backend.face.max_image_bytes={limit}",
                    bytes = entry.size,
                    limit = face_cfg.max_image_bytes,
                ),
            );
            record_failure(report, entry.location.display(), &err);
            continue;
        }
        let Some(scanned) = scan_entry(src_backend, &entry.location, source, report) else {
            continue;
        };
        out.push(scanned);
    }
}

/// 单文件 scan：读字节 → MIME 嗅探 → decode → pHash + sharpness 共享 `Arc`。
fn scan_entry(
    src_backend: &Arc<dyn Backend>,
    location: &Location,
    source: &Location,
    report: &mut CullReport,
) -> Option<ScannedFile> {
    let bytes = match read_all(src_backend, location) {
        Ok(b) => b,
        Err(e) => {
            record_failure(report, location.display(), &e);
            return None;
        }
    };
    if !is_image(&bytes) {
        return None;
    }
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            record_failure(
                report,
                location.display(),
                &io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
            );
            return None;
        }
    };
    let hash = phash(&img);
    let luma = image::imageops::grayscale(&DynamicImage::ImageRgb8(img.clone()));
    let sharp = laplacian_variance(&luma);
    Some(ScannedFile {
        src_loc: location.clone(),
        src_backend: src_backend.clone(),
        source_root: source.clone(),
        hash,
        sharpness: sharp,
        raw_bytes: Arc::new(bytes),
        decoded: Arc::new(img),
    })
}

/// 组内逐图跑 4 模型 + `face_scoring`，选 `breakdown.total` 最高者；同时调
/// `identity_cluster` 输出跨图身份簇日志（不影响选择，留作未来 per-identity 策略接入点）。
#[expect(
    clippy::too_many_arguments,
    reason = "组评分接 4 detector + cfg + report：调用单点不再拆"
)]
fn pick_best_for_group(
    indices: &[usize],
    scanned: &[ScannedFile],
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    report: &mut CullReport,
) -> (usize, ScoreBreakdown) {
    let mut best_idx = indices[0];
    let mut best_total = f32::NEG_INFINITY;
    let mut best_breakdown = ScoreBreakdown::default();
    let mut per_image_embeddings: Vec<Vec<[f32; identity_cluster::EMBED_DIM]>> =
        Vec::with_capacity(indices.len());
    for &i in indices {
        let item = &scanned[i];
        let Some(analysis) = analyze_image(item, scrfd, facenet, facemesh, eyestate, report) else {
            per_image_embeddings.push(Vec::new());
            continue;
        };
        per_image_embeddings.push(analysis.embeddings.clone());
        let breakdown = face_scoring::score_image(
            item.sharpness,
            &analysis.faces,
            &analysis.meshes,
            &analysis.eye_states,
            face_cfg,
        );
        if breakdown.total > best_total {
            best_total = breakdown.total;
            best_idx = i;
            best_breakdown = breakdown;
        }
    }
    let clusters =
        identity_cluster::cluster_identities(&per_image_embeddings, face_cfg.face_cosine_min);
    log_identity_clusters(&clusters);
    // 没有任一 analysis 成功（best_total 仍是 NEG_INFINITY）→ best_breakdown 全 0，
    // best_idx 是 indices[0] 默认值；上游 write_group 仍按此搬剩余 culled。
    if best_total.is_finite() {
        return (best_idx, best_breakdown);
    }
    // 兜底：用 indices[0] 的 sharpness 作 total，让 report 显示有意义值。
    let fallback = &scanned[best_idx];
    (
        best_idx,
        ScoreBreakdown {
            sharpness: fallback.sharpness,
            blink_penalty: 0.0,
            smile_bonus: 0.0,
            total: fallback.sharpness,
        },
    )
}

/// 单图 4 模型印证：`SCRFD` → 每脸 (`face_align` → `facenet`) + (bbox crop → `facemesh`) +
/// (eye crop → `eyestate`)。`SCRFD` Err 整图记 failure 返 None；单脸 `face_align`/`facenet`
/// Err 整脸丢弃；`facemesh`/`eyestate` Err 退化为空 mesh / 0 闭眼概率（不丢脸）。
fn analyze_image(
    item: &ScannedFile,
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    report: &mut CullReport,
) -> Option<ImageAnalysis> {
    let detections = match scrfd.detect_faces(item.src_loc.path(), &item.raw_bytes) {
        Ok(f) => f,
        Err(e) => {
            record_failure(report, item.src_loc.display(), &e);
            return None;
        }
    };
    let mut analysis = ImageAnalysis {
        faces: Vec::with_capacity(detections.len()),
        embeddings: Vec::with_capacity(detections.len()),
        meshes: Vec::with_capacity(detections.len()),
        eye_states: Vec::with_capacity(detections.len()),
    };
    for face in &detections {
        let Ok(aligned) = face_align::align_face(&item.decoded, &face.landmarks_5pt) else {
            continue;
        };
        let Ok(embedding) = facenet.embed_face(item.src_loc.path(), &aligned) else {
            continue;
        };
        let mesh = facemesh
            .detect_mesh(item.src_loc.path(), &crop_face_bbox(&item.decoded, face))
            .unwrap_or_default();
        let eye_pair = classify_eye_pair(item, face, eyestate);
        analysis.faces.push(*face);
        analysis.embeddings.push(embedding);
        analysis.meshes.push(mesh);
        analysis.eye_states.push(eye_pair);
    }
    Some(analysis)
}

/// 用 SCRFD bbox 从原图裁出人脸区域（clamp 到图像边界）。空 bbox 返 1×1 占位让下游不 panic。
fn crop_face_bbox(image: &RgbImage, face: &FaceDetection) -> RgbImage {
    let w = image.width();
    let h = image.height();
    let x0 = face.bbox[0].max(0.0).round();
    let y0 = face.bbox[1].max(0.0).round();
    let x1 = face.bbox[2].max(0.0).round();
    let y1 = face.bbox[3].max(0.0).round();
    let xu = u32_from_f32_clamped(x0, w);
    let yu = u32_from_f32_clamped(y0, h);
    let xe = u32_from_f32_clamped(x1, w);
    let ye = u32_from_f32_clamped(y1, h);
    if xe <= xu || ye <= yu {
        return RgbImage::new(1, 1);
    }
    image::imageops::crop_imm(image, xu, yu, xe - xu, ye - yu).to_image()
}

/// 用 SCRFD 5 点的左/右眼坐标各 crop 一个方形眼区域调 EyeState，返左/右闭眼概率对。
fn classify_eye_pair(
    item: &ScannedFile,
    face: &FaceDetection,
    eyestate: &dyn EyeStateClassifier,
) -> (f32, f32) {
    let bbox_h = (face.bbox[3] - face.bbox[1]).max(1.0);
    let radius = (bbox_h * EYE_CROP_RADIUS_RATIO).round();
    let left_crop = crop_eye_around(&item.decoded, face.landmarks_5pt[0], radius);
    let right_crop = crop_eye_around(&item.decoded, face.landmarks_5pt[1], radius);
    let left = eyestate
        .classify_eye(item.src_loc.path(), &left_crop)
        .unwrap_or(0.0);
    let right = eyestate
        .classify_eye(item.src_loc.path(), &right_crop)
        .unwrap_or(0.0);
    (left, right)
}

fn crop_eye_around(image: &RgbImage, center: [f32; 2], radius: f32) -> RgbImage {
    let w = image.width();
    let h = image.height();
    let cx = center[0];
    let cy = center[1];
    let x0 = u32_from_f32_clamped(cx - radius, w);
    let y0 = u32_from_f32_clamped(cy - radius, h);
    let x1 = u32_from_f32_clamped(cx + radius, w);
    let y1 = u32_from_f32_clamped(cy + radius, h);
    if x1 <= x0 || y1 <= y0 {
        return RgbImage::new(1, 1);
    }
    image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image()
}

/// `f32` 像素坐标 clamp 到 `[0, limit]` 并安全转 `u32`。NaN/超限 → 0 / limit。
fn u32_from_f32_clamped(v: f32, limit: u32) -> u32 {
    if !v.is_finite() || v < 0.0 {
        return 0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "limit ≤ 图像宽高 < 65536 << f32 mantissa 边界"
    )]
    let limit_f = limit as f32;
    let clamped = v.min(limit_f);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "上行已 clamp 到 [0, limit_f]，u32 cast 安全"
    )]
    let u = clamped as u32;
    u
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
