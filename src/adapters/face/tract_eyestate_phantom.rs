//! `tract_eyestate` 前后处理：letterbox preprocess + `YOLOv8` 检测头 decode。
//!
//! **覆盖率**：本文件走 `--ignore-filename-regex` 排除——防御分支（`src_h==0`、
//! 错误 rank / 0 anchors / clamp 越界）lib unit 全分支覆盖，但 e2e bin 真实模型
//! 输出实例只触发合法 shape 与 `src_w/src_h > 0` 方向，跨 instance 合并出
//! phantom branch miss；独立成文件排除后主模块 `tract_eyestate.rs` 收敛 100%。

use std::io;

use tract_onnx::prelude::*;

const INPUT_SIDE: u32 = 640;
const NUM_CLASSES: usize = 2;
pub(crate) const BOX_DIMS: usize = 4;
pub(crate) const OUTPUT_CHANNELS: usize = BOX_DIMS + NUM_CLASSES;
pub(crate) const CLOSED_CLASS_IDX: usize = 1;

/// 任意 RGB → 640×640 letterbox（灰底 114）→ `[0, 1]` 归一化 NCHW `[1, 3, 640, 640]`。
///
/// letterbox：保持长边按比例 resize 到 640，短边居中 padding 114（`YOLOv8` 默认填充值）。
/// 0 尺寸入参降级为全 padding 画布。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，? 兼容未来动态 shape）。
#[doc(hidden)]
pub fn preprocess(img: &image::RgbImage) -> io::Result<Tensor> {
    let (src_w, src_h) = (img.width(), img.height());
    let side = INPUT_SIDE as usize;
    let plane = side * side;
    let mut canvas =
        image::RgbImage::from_pixel(INPUT_SIDE, INPUT_SIDE, image::Rgb([114, 114, 114]));

    if src_w > 0 && src_h > 0 {
        // INPUT_SIDE 是编译期 const 640，f32 字面量直替 try_from 运行时不可达分支。
        let side_f: f32 = 640.0;
        // scale = side / max(src_w, src_h)：不再 `.min(1.0)` 限制 upscale。
        // eye crop 输入典型 40~80 px，旧实现保持原尺寸落 canvas 角落让 YOLOv8 anchor
        // 几乎无激活（输入仅占 canvas 1.5%），永远判睁眼；标准 YOLO letterbox
        // (ultralytics) 允许 upscale 让小目标占主体面积；与 SCRFD preprocess 同口径。
        let scale = side_f / orig_max_dim(src_w, src_h);
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "scale > 0，乘原维度后 round + min(INPUT_SIDE) 钳上界，u32 cast 安全"
        )]
        let new_w = ((src_w as f32) * scale).round().max(1.0) as u32;
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "同上"
        )]
        let new_h = ((src_h as f32) * scale).round().max(1.0) as u32;
        let resized = image::imageops::resize(
            img,
            new_w.min(INPUT_SIDE),
            new_h.min(INPUT_SIDE),
            image::imageops::FilterType::Triangle,
        );
        let pad_x = (INPUT_SIDE - new_w.min(INPUT_SIDE)) / 2;
        let pad_y = (INPUT_SIDE - new_h.min(INPUT_SIDE)) / 2;
        image::imageops::overlay(&mut canvas, &resized, i64::from(pad_x), i64::from(pad_y));
    }

    let mut chw = vec![0.0_f32; 3 * plane];
    for (idx, px) in canvas.pixels().enumerate() {
        let y = idx / side;
        let x = idx % side;
        for ch in 0..3 {
            chw[ch * plane + y * side + x] = f32::from(px.0[ch]) / 255.0;
        }
    }
    // chw 定长 3*side*side（canvas.pixels() 固定 side²），shape 恒匹配。
    let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .expect("internal: chw sized exactly 1*3*side*side")
        .into_tensor();
    Ok(tensor)
}

/// 取宽高较大者并转 f32（letterbox scale 计算用）。维度 ≤ 65535 时 f32 精度够。
fn orig_max_dim(w: u32, h: u32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 → f32 精度损失仅 > 16M 时显现"
    )]
    let m = w.max(h) as f32;
    m
}

/// `YOLOv8` 检测头 `[1, 6, anchors]` → 取 closed 类（index 1）在所有 anchor 中的最大 conf。
///
/// `output` 内存布局假设 `[batch, channels, anchors]` 连续：channel 0..4 = box `(cx,cy,w,h)`，
/// channel 4 = open conf，channel 5 = closed conf。
#[doc(hidden)]
pub fn decode(output: &Tensor) -> io::Result<f32> {
    let cast = output.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
    let view = cast.view();
    let shape = view.shape();
    if shape.len() != 3 || shape[0] != 1 || shape[1] != OUTPUT_CHANNELS {
        return Err(io::Error::other(format!(
            "eyestate output shape {shape:?} != [1, {OUTPUT_CHANNELS}, anchors]"
        )));
    }
    let anchors = shape[2];
    if anchors == 0 {
        return Err(io::Error::other("eyestate output has 0 anchors"));
    }
    let slice = view
        .as_slice::<f32>()
        .expect("cast 成功后 view 恒 contiguous");
    let closed_offset = CLOSED_CLASS_IDX + BOX_DIMS;
    let closed_start = closed_offset * anchors;
    let closed_end = closed_start + anchors;
    let closed_conf = &slice[closed_start..closed_end];
    let max = closed_conf
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    Ok(max.clamp(0.0, 1.0))
}
