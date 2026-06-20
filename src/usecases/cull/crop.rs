//! `cull` 几何/数值 helper：bbox / eye crop + u32 clamp + NaN 比较。
//! 纯函数无 `ScannedFile` 依赖，外置以让 `run.rs` 保 ≤ 512 行（P0 §7）。

use image::RgbImage;

use crate::usecases::face::FaceDetection;

/// 用 SCRFD bbox 从原图裁出人脸区域（clamp 到图像边界）。空 bbox 返 1×1 占位让下游不 panic。
pub(super) fn crop_face_bbox(image: &RgbImage, face: &FaceDetection) -> RgbImage {
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

/// 围绕 `center` 按 `radius` 方形 crop。`center` 越界或 `radius` 退化时返 1×1。
pub(super) fn crop_eye_around(image: &RgbImage, center: [f32; 2], radius: f32) -> RgbImage {
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

/// `f32` 像素坐标 clamp 到 `[0, limit]` 并安全转 `u32`。
/// NaN → 0（无意义），负数 → 0（下边界），+Inf → limit（上边界 clamp 而非视作无效）。
pub(super) fn u32_from_f32_clamped(v: f32, limit: u32) -> u32 {
    if v.is_nan() || v < 0.0 {
        return 0;
    }
    if v.is_infinite() {
        return limit;
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

/// `partial_cmp` NaN 返 None 让 `max_by` 退化为 Equal 取末尾。把 NaN 视为 `-∞`：
/// NaN vs finite → Less（finite 胜）；finite vs NaN → Greater；NaN vs NaN → Equal。
/// 同口径让 score=NaN 的图不会意外被选为 best。
pub(super) fn total_cmp_nan_as_neg_inf(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
    }
}
