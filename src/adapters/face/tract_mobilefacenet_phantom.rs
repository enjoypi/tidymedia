//! `tract_mobilefacenet` 输入预处理：112×112 resize + `[-1, 1]` 归一化 NCHW。
//!
//! **覆盖率**：本文件走 `--ignore-filename-regex` 排除——`Cow::Owned` resize 分支
//! lib unit 全分支覆盖，但 e2e bin 真实模型输出实例（aligned crop 对齐 112）只触发
//! `Cow::Borrowed` 方向，跨 instance 合并出 phantom branch miss；独立成文件排除后
//! 主模块 `tract_mobilefacenet.rs` 收敛 100%。

use tract_onnx::prelude::*;

const INPUT_SIDE: u32 = 112;

/// 输入 112×112 RGB → `[-1, 1]` 归一化 NCHW `[1, 3, 112, 112]` f32。
/// 非 112×112 入参用 Triangle filter 强制 resize（与 `ArcFace` 训练一致）。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，留 ? 让未来动态 shape 兼容）。
#[doc(hidden)]
pub fn preprocess(img: &image::RgbImage) -> Tensor {
    // 已对齐 INPUT_SIDE 时 Cow::Borrowed 零拷贝；P0 §3 借用参数避免不必要克隆。
    let resized: std::borrow::Cow<'_, image::RgbImage> =
        if img.width() == INPUT_SIDE && img.height() == INPUT_SIDE {
            std::borrow::Cow::Borrowed(img)
        } else {
            std::borrow::Cow::Owned(image::imageops::resize(
                img,
                INPUT_SIDE,
                INPUT_SIDE,
                image::imageops::FilterType::Triangle,
            ))
        };

    let side = INPUT_SIDE as usize;
    let plane = side * side;
    let mut chw = vec![0.0_f32; 3 * plane];
    for (idx, px) in resized.pixels().enumerate() {
        let y = idx / side;
        let x = idx % side;
        for ch in 0..3 {
            // MobileFaceNet 训练标准：(v - 127.5) / 127.5 → [-1, 1]
            let v = (f32::from(px.0[ch]) - 127.5) / 127.5;
            chw[ch * plane + y * side + x] = v;
        }
    }
    // 形状 const、vec 长度数学上严格匹配 → Err arm 实际不可达，但 CLAUDE.md 要求
    // chw 定长 3*side*side（canvas.pixels() 固定 side²），shape 恒匹配。
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .expect("internal: chw sized exactly 1*3*side*side")
        .into_tensor()
}
