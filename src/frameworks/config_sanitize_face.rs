// FaceConfig 阈值/权重 sanitize：非法字段值回退默认并告警，与 `sanitize_ocr` 同哲学。
// 拆自 `config_sanitize.rs`（原 323 行 → 主文件 + face 两文件）。
use tracing::warn;

use crate::usecases::config::FaceConfig;

// FaceConfig 各阈值/权重越界即 warn + 回退默认，同 `sanitize_ocr` 哲学：
// 配置错误不让 cull 子命令静默全失败，但必须可观测。
// - `phash_hamming_max ∈ [1, 64]`：0 让所有图不分组、>64 让全图集成一大组
// - `sharpness_min > 0` 有限值：≤0 关粗筛、NaN/Inf 让 `<` 比较全 false 让所有图都过
// - 比例阈值（cosine/EAR/EyeState）∈ (0,1)：越界让判定恒真/恒假
// - 评分权重 `w_*` 必须有限非负：负值反转语义、NaN 让 score 全 NaN
pub(super) fn sanitize_face(face: &mut FaceConfig) {
    const MAX_HAMMING: u8 = 64;

    let defaults = FaceConfig::default();
    if face.phash_hamming_max == 0 || face.phash_hamming_max > MAX_HAMMING {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.face.phash_hamming_max",
            value = face.phash_hamming_max,
            fallback = defaults.phash_hamming_max,
            "phash_hamming_max must be in [1, 64]; falling back to default"
        );
        face.phash_hamming_max = defaults.phash_hamming_max;
    }
    if !face.sharpness_min.is_finite() || face.sharpness_min <= 0.0 {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.face.sharpness_min",
            value = face.sharpness_min,
            fallback = defaults.sharpness_min,
            "sharpness_min must be a finite positive number; falling back to default"
        );
        face.sharpness_min = defaults.sharpness_min;
    }
    sanitize_face_unit_open(
        &mut face.scrfd_score_threshold,
        defaults.scrfd_score_threshold,
        "backend.face.scrfd_score_threshold",
    );
    sanitize_face_unit_open(
        &mut face.scrfd_nms_iou,
        defaults.scrfd_nms_iou,
        "backend.face.scrfd_nms_iou",
    );
    sanitize_face_unit_open(
        &mut face.face_cosine_min,
        defaults.face_cosine_min,
        "backend.face.face_cosine_min",
    );
    sanitize_face_unit_open(
        &mut face.ear_blink_max,
        defaults.ear_blink_max,
        "backend.face.ear_blink_max",
    );
    sanitize_face_unit_open(
        &mut face.eye_blink_score_max,
        defaults.eye_blink_score_max,
        "backend.face.eye_blink_score_max",
    );
    sanitize_face_unit_open(
        &mut face.eye_crop_radius_ratio,
        defaults.eye_crop_radius_ratio,
        "backend.face.eye_crop_radius_ratio",
    );
    sanitize_face_weight(
        &mut face.w_sharpness,
        defaults.w_sharpness,
        "backend.face.w_sharpness",
    );
    sanitize_face_weight(&mut face.w_blink, defaults.w_blink, "backend.face.w_blink");
    sanitize_face_weight(&mut face.w_smile, defaults.w_smile, "backend.face.w_smile");
    super::sanitize_max_image_bytes_field(
        &mut face.max_image_bytes,
        defaults.max_image_bytes,
        "backend.face.max_image_bytes",
    );
}

pub(super) fn sanitize_face_unit_open(value: &mut f32, fallback: f32, field: &str) {
    if !super::is_unit_open(*value) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            value = *value,
            fallback,
            "value must be in (0, 1); falling back to default"
        );
        *value = fallback;
    }
}

pub(super) fn sanitize_face_weight(value: &mut f32, fallback: f32, field: &str) {
    if !value.is_finite() || *value < 0.0 {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            value = *value,
            fallback,
            "weight must be a finite non-negative number; falling back to default"
        );
        *value = fallback;
    }
}
