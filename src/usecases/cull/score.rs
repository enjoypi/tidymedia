//! cull 组级评分：组内逐图跑 4 模型印证 → `face_scoring::score_image` →
//! 选 `breakdown.total` 最高者；同时调 `identity_cluster` 输出跨图身份簇日志。
//! 并行 ONNX 评分（tract `TypedRunnableModel::run(&self,...)` 支持 `&self` 并发）。

use std::io;

use image::RgbImage;

use super::crop::{crop_eye_around, crop_face_bbox, total_cmp_nan_as_neg_inf};
use super::face_align;
use super::face_scoring;
use super::identity_cluster;
use super::report::ScoreBreakdown;
use super::scan::ScannedFile;
use super::util::{log_analyze_image, log_identity_clusters, log_pick_best, read_all};
use crate::usecases::config::FaceConfig;
use crate::usecases::face::{
    EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector,
};

/// 单张图 4 模型印证结果（faces 长度与其余 3 vec 一致：对齐失败/嵌入失败的 face 整体丢弃）。
pub(super) struct ImageAnalysis {
    faces: Vec<FaceDetection>,
    embeddings: Vec<[f32; identity_cluster::EMBED_DIM]>,
    meshes: Vec<Vec<[f32; 3]>>,
    eye_states: Vec<(f32, f32)>,
}

/// 组内逐图跑 4 模型 + `face_scoring`，选 `breakdown.total` 最高者；同时调
/// `identity_cluster` 输出跨图身份簇日志（不影响选择，留作未来 per-identity 策略接入点）。
#[expect(
    clippy::too_many_arguments,
    reason = "组评分接 4 detector + cfg + failures：调用单点不再拆"
)]
pub(super) fn pick_best_for_group(
    indices: &[usize],
    scanned: &[ScannedFile],
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    failures: &mut Vec<(String, io::Error)>,
) -> (usize, ScoreBreakdown, Vec<ScoreBreakdown>) {
    // 每个 indices 项总是有 breakdown：analyze_image 失败时退化为 sharpness-only
    // 计算（face_count=0 时 score_image 仅含 w_sharpness*sharpness 项）。这样 culled
    // 项的 score 字段也用 breakdown.total，与 best 的 score_breakdown.total 同口径，
    // 不再混用 sharpness 单分量（破坏 CulledEntry「综合评分」承诺）。
    let mut breakdowns: Vec<ScoreBreakdown> = Vec::with_capacity(indices.len());
    let mut per_image_embeddings: Vec<Vec<[f32; identity_cluster::EMBED_DIM]>> =
        Vec::with_capacity(indices.len());
    for &i in indices {
        let item = &scanned[i];
        let analysis = analyze_image(item, scrfd, facenet, facemesh, eyestate, face_cfg, failures);
        let (faces, meshes, eye_states, embeddings) = match analysis {
            Some(a) => (a.faces, a.meshes, a.eye_states, a.embeddings),
            None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        };
        per_image_embeddings.push(embeddings);
        breakdowns.push(face_scoring::score_image(
            item.sharpness,
            &faces,
            &meshes,
            &eye_states,
            face_cfg,
        ));
    }
    // TODO: per-identity 策略接入：clusters 当前仅产 debug 日志，pick_best 按全组
    // max(total) 选 best 不区分身份；若需「同人多张里选最佳 + 不同人各自保留」语义，
    // 在此按 clusters 分桶再于每桶内 max_by_key 取首张 → 当前 face_cosine_min 才有效。
    let clusters =
        identity_cluster::cluster_identities(&per_image_embeddings, face_cfg.face_cosine_min);
    log_identity_clusters(&clusters);

    // 选最高 total；NaN 视为 -∞ 让 NaN total 永远输给 finite 同分 → max_by 在
    // 全 finite 同分时 Rust 标准取末尾元素，配 `>` 严格比较保稳定（同 total 取首张
    // 即「先扫描的更优」直觉）；NaN 同 NaN 视为 Equal，返首个 NaN。
    // indices.len() >= 2（调用方保证）+ breakdowns 同长 → ok_or_else 兜底返
    // 第 0 项 breakdown 防 caller-contract 失守。
    let best_pos = breakdowns
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| total_cmp_nan_as_neg_inf(a.total, b.total))
        .map_or(0, |(p, _)| p);
    let best_idx = indices[best_pos];
    let best_breakdown = breakdowns[best_pos];
    // P0 §14 业务 debug：组规模 + 选中分数。
    log_pick_best(indices.len(), best_idx, best_breakdown.total);
    (best_idx, best_breakdown, breakdowns)
}

/// 单图 4 模型印证：按需重读字节 + 重 decode → `SCRFD` → 每脸 (`face_align` → `facenet`)
/// + (bbox crop → `facemesh`) + (eye crop → `eyestate`)。
///
/// 重读+重 decode 是 OOM 修复（scan 阶段不再缓存 `raw_bytes`/`decoded`）：仅多图组成员承担
/// 二次开销，单图组在 caller 已跳过。
///
/// `read_all`/`load_from_memory` Err 与 `SCRFD` Err 整图记 failure 返 None；
/// 单脸 `face_align`/`facenet` Err 整脸丢弃；`facemesh`/`eyestate` Err 退化为空 mesh
/// / 0 闭眼概率（不丢脸）。
pub(super) fn analyze_image(
    item: &ScannedFile,
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    failures: &mut Vec<(String, io::Error)>,
) -> Option<ImageAnalysis> {
    let bytes = match read_all(&item.src_backend, &item.src_loc) {
        Ok(b) => b,
        Err(e) => {
            failures.push((item.src_loc.display(), e));
            return None;
        }
    };
    let decoded = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            failures.push((
                item.src_loc.display(),
                io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
            ));
            return None;
        }
    };
    let detections = match scrfd.detect_faces(item.src_loc.path(), &bytes) {
        Ok(f) => f,
        Err(e) => {
            failures.push((item.src_loc.display(), e));
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
        let Ok(aligned) = face_align::align_face(&decoded, &face.landmarks_5pt) else {
            continue;
        };
        let Ok(embedding) = facenet.embed_face(item.src_loc.path(), &aligned) else {
            continue;
        };
        let mesh = facemesh
            .detect_mesh(item.src_loc.path(), &crop_face_bbox(&decoded, face))
            .unwrap_or_default();
        let eye_pair = classify_eye_pair(
            item,
            &decoded,
            face,
            eyestate,
            face_cfg.eye_crop_radius_ratio,
        );
        analysis.faces.push(*face);
        analysis.embeddings.push(embedding);
        analysis.meshes.push(mesh);
        analysis.eye_states.push(eye_pair);
    }
    // P0 §14 ONNX 外部调用 debug：SCRFD 检测到的脸数 + 成功 embed/对齐的脸数（req=image，resp=faces）。
    log_analyze_image(
        &item.src_loc.display(),
        detections.len(),
        analysis.faces.len(),
    );
    Some(analysis)
}

/// 用 SCRFD 5 点的左/右眼坐标各 crop 一个方形眼区域调 EyeState，返左/右闭眼概率对。
fn classify_eye_pair(
    item: &ScannedFile,
    decoded: &RgbImage,
    face: &FaceDetection,
    eyestate: &dyn EyeStateClassifier,
    eye_crop_radius_ratio: f32,
) -> (f32, f32) {
    let bbox_h = (face.bbox[3] - face.bbox[1]).max(1.0);
    let radius = (bbox_h * eye_crop_radius_ratio).round();
    let left_crop = crop_eye_around(decoded, face.landmarks_5pt[0], radius);
    let right_crop = crop_eye_around(decoded, face.landmarks_5pt[1], radius);
    let left = eyestate
        .classify_eye(item.src_loc.path(), &left_crop)
        .unwrap_or(0.0);
    let right = eyestate
        .classify_eye(item.src_loc.path(), &right_crop)
        .unwrap_or(0.0);
    (left, right)
}
