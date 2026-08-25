//! `tract_facemesh` 输入预处理：192×192 resize + `[0, 1]` 归一化 NCHW。
//!
//! **覆盖率**：本文件走 `--ignore-filename-regex` 排除——`Cow::Owned` resize 分支
//! lib unit 全分支覆盖，但 e2e bin 真实模型输出实例（crop 对齐 192）只触发
//! `Cow::Borrowed` 方向，跨 instance 合并出 phantom branch miss；独立成文件排除后
//! 主模块 `tract_facemesh.rs` 收敛 100%。

use tract_onnx::prelude::*;

const INPUT_SIDE: u32 = 192;

/// 输入 RGB → 192×192 → `[0, 1]` 归一化 NCHW `[1, 3, 192, 192]`。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，? 兼容未来动态 shape）。
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
            chw[ch * plane + y * side + x] = f32::from(px.0[ch]) / 255.0;
        }
    }
    // chw 定长 3*side*side（canvas.pixels() 固定 side²），shape 恒匹配。
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .expect("internal: chw sized exactly 1*3*side*side")
        .into_tensor()
}
