//! 综合评分：清晰度 - 闭眼惩罚 + 微笑加分。
//!
//! 闭眼**双印证**：`MediaPipe` `FaceMesh` 几何 `EAR`（`Eye Aspect Ratio`）<
//! `ear_blink_max` **或** `YOLOv8` `EyeState` 闭眼概率 > `eye_blink_score_max`，
//! 任一命中即判闭眼，记 1 次惩罚。这样硬件感知（`EyeState`）与几何（`EAR`）
//! 互补——`SCRFD` 5 点对齐误差让 `EAR` 退化时还有 `EyeState` 兜底，反之亦然。
//!
//! 微笑评分：`MediaPipe` 4 点（嘴角 61/291 + 上下唇中 13/14）算嘴角相对嘴中心的
//! 上扬幅度，按嘴宽归一化，只取非负值（轻微下垂不变负惩罚）。
//!
//! `EAR` 6 点索引依 `MediaPipe` 标准（`face_landmarker.task` 468 点 mesh）：
//! - 左眼：33（外角）/ 160 / 158 / 133（内角）/ 153 / 144
//! - 右眼：362（外角）/ 385 / 387 / 263（内角）/ 373 / 380

// pick_best 接入 4 模型印证流水线前本模块仅被单测调用。commit 5 把
// score_image 接入 run.rs 后此 allow 删除。
#![allow(dead_code, reason = "占位实现：pick_best 接入 4 模型印证后启用")]

use crate::adapters::face::FaceDetection;
use crate::usecases::config::FaceConfig;

use super::report::ScoreBreakdown;

/// `MediaPipe` `FaceMesh` 输出 468 点（部分变体 478 含虹膜）；不足此数视为退化跳过。
const MESH_POINT_COUNT: usize = 468;

/// 左/右眼 6 点 EAR 索引（`MediaPipe` 标准）。
const LEFT_EYE_IDX: [usize; 6] = [33, 160, 158, 133, 153, 144];
const RIGHT_EYE_IDX: [usize; 6] = [362, 385, 387, 263, 373, 380];

/// 嘴部 4 关键点：左嘴角 / 右嘴角 / 上唇中 / 下唇中。
const MOUTH_LEFT: usize = 61;
const MOUTH_RIGHT: usize = 291;
const LIP_UPPER: usize = 13;
const LIP_LOWER: usize = 14;

/// 综合 4 模型输出与全图清晰度算 `ScoreBreakdown`。
///
/// `meshes` / `eye_states` 长度应等于 `faces.len()`；不足部分按"无 mesh / 无
/// `EyeState`"对应脸跳过（不计 `EAR` 也不计 `EyeState` 印证）。同张脸 `EAR` 与
/// `EyeState` 都命中只计 1 次惩罚。
pub(crate) fn score_image(
    sharpness: f32,
    faces: &[FaceDetection],
    meshes: &[Vec<[f32; 3]>],
    eye_states: &[(f32, f32)],
    cfg: &FaceConfig,
) -> ScoreBreakdown {
    let face_count = faces.len();
    let mut blink_faces: f32 = 0.0;
    let mut smile_sum: f32 = 0.0;
    let mut smile_face_count: f32 = 0.0;
    for i in 0..face_count {
        let ear_closed = meshes
            .get(i)
            .and_then(|m| ear_from_mesh(m))
            .is_some_and(|ear| ear < cfg.ear_blink_max);
        let eye_closed = eye_states
            .get(i)
            .is_some_and(|(left, right)| left.max(*right) > cfg.eye_blink_score_max);
        if ear_closed || eye_closed {
            blink_faces += 1.0;
        }
        if let Some(smile) = meshes.get(i).and_then(|m| smile_from_mesh(m)) {
            smile_sum += smile;
            smile_face_count += 1.0;
        }
    }
    let smile_avg = if smile_face_count > 0.0 {
        smile_sum / smile_face_count
    } else {
        0.0
    };
    let blink_penalty = cfg.w_blink * blink_faces;
    let smile_bonus = cfg.w_smile * smile_avg;
    let total = cfg
        .w_sharpness
        .mul_add(sharpness, -blink_penalty + smile_bonus);
    ScoreBreakdown {
        sharpness,
        blink_penalty,
        smile_bonus,
        total,
    }
}

/// 左右眼 `EAR` 平均。mesh 不足 468 点 / 索引退化（眼宽=0）返 `None` 让调用方跳过印证。
fn ear_from_mesh(mesh: &[[f32; 3]]) -> Option<f32> {
    if mesh.len() < MESH_POINT_COUNT {
        return None;
    }
    let left = ear_at_indices(mesh, LEFT_EYE_IDX)?;
    let right = ear_at_indices(mesh, RIGHT_EYE_IDX)?;
    Some(f32::midpoint(left, right))
}

/// 单眼 `EAR` = `(d(p2,p6) + d(p3,p5)) / (2·d(p1,p4))`。水平距离 ≈ 0 返 `None`。
fn ear_at_indices(mesh: &[[f32; 3]], idx: [usize; 6]) -> Option<f32> {
    let pt = |i: usize| [mesh[idx[i]][0], mesh[idx[i]][1]];
    let p1 = pt(0);
    let p2 = pt(1);
    let p3 = pt(2);
    let p4 = pt(3);
    let p5 = pt(4);
    let p6 = pt(5);
    let horiz = dist_2d(p1, p4);
    if horiz < f32::EPSILON {
        return None;
    }
    Some((dist_2d(p2, p6) + dist_2d(p3, p5)) / (2.0 * horiz))
}

/// 微笑分 = `(嘴角上扬 / 嘴宽)`，向上扬为正，下垂或嘴宽 0 返 0。
fn smile_from_mesh(mesh: &[[f32; 3]]) -> Option<f32> {
    if mesh.len() < MESH_POINT_COUNT {
        return None;
    }
    let lc = mesh[MOUTH_LEFT];
    let rc = mesh[MOUTH_RIGHT];
    let upper = mesh[LIP_UPPER];
    let lower = mesh[LIP_LOWER];
    let center_y = f32::midpoint(upper[1], lower[1]);
    let mouth_width = (rc[0] - lc[0]).abs();
    if mouth_width < f32::EPSILON {
        return Some(0.0);
    }
    // 图像坐标 y 向下增大；嘴角"上扬"即 corner.y < center_y → curl 为负 → smile > 0。
    let curl = f32::midpoint(lc[1] - center_y, rc[1] - center_y);
    Some((-curl / mouth_width).max(0.0))
}

fn dist_2d(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx.hypot(dy)
}

#[cfg(test)]
#[path = "face_scoring_tests.rs"]
mod tests;
