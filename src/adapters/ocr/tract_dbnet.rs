//! tract-onnx 实现的 `TextDetector`：跑 `PaddleOCR` `DBNet` `det.onnx` 仅判「有/无文本」。
//!
//! 设计：
//! - **懒加载**：首次 `has_text` 触发模型加载（`OnceLock<Result<...>>` 风格——但 stable
//!   Rust 不支持 `get_or_try_init`，用 `Mutex<Option<...>>` 兼容；模型加载是 idempotent
//!   读 ONNX 文件，竞态首次加载多次也无副作用）
//! - **极简后处理**：sigmoid 输出图中 `v > binarize_threshold` 像素占比 > `min_text_pixel_ratio`
//!   即判命中。**跳过 polygon contour**（不取文本框坐标，本子命令只需二元判定）
//! - **真实模型加载隔离**：`tract_dbnet_real::*` 拆出整文件让 `--ignore-filename-regex='_real\.rs$'`
//!   排除（CI 无模型不可触发）；本文件只装配 + 前/后处理 + fake 注入测试

use std::io;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8Path;
use image::GenericImageView;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::TextDetector;
use super::tract_dbnet_real::load_runnable;
use crate::usecases::config::OcrConfig;

/// 已优化并固定 shape 的 `DBNet` 推理图；`Arc` 让 trait 对象多线程共享。
/// `TypedRunnableModel` 是 tract `RunnableModel<TypedFact, Box<dyn TypedOp>>` 的别名。
pub(crate) type DetModel = Arc<TypedRunnableModel>;

/// 把 OCR 推理拆成「模型加载」+「单张推理」两步注入：测试可注入 stub model 直接
/// 验前/后处理；生产路径走 tract 真实加载（`tract_dbnet_real`）。trait 对象安全。
pub(crate) trait RawDetector: Send + Sync {
    /// 接收预处理后的 NCHW [1, 3, H, W] 张量；返回与之同 H/W 的 [1, 1, H, W] sigmoid 图。
    ///
    /// # Errors
    ///
    /// 当模型推理失败、输出维度不符预期时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

/// 真实 tract `RunnableModel` 适配为 `RawDetector`：仅薄包一层，让前/后处理在
/// 本文件保持单元测试可达，真实模型路径走 `_real.rs`（CI 不触发）。
///
/// 本 struct 与 `run` impl 仅在真实 ONNX 模型存在时才被构造（`ensure_raw` 中），
/// CI 环境无模型文件不可触发；`run` 加 `coverage(off)`，struct 自身字段
/// 由 `ensure_raw` 的 `coverage(off)` 间接排除（attribute 不能加在 struct 上）。
struct TractRawDetector {
    model: DetModel,
}

impl RawDetector for TractRawDetector {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract DBNet run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("tract DBNet returned no output tensor"))?;
        Ok(first.into_tensor())
    }
}

/// Detector 主体：持有 OCR 配置 + 懒加载模型。`Mutex<Option<...>>` 替代 nightly 的
/// `OnceLock::get_or_try_init`；模型加载是 idempotent，竞态首次加载多次无副作用。
pub struct TractDbnetDetector {
    cfg: OcrConfig,
    raw: Mutex<Option<Box<dyn RawDetector>>>,
}

impl std::fmt::Debug for TractDbnetDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractDbnetDetector")
            .field("det_model_path", &self.cfg.det_model_path)
            .field("binarize_threshold", &self.cfg.binarize_threshold)
            .field("min_text_pixel_ratio", &self.cfg.min_text_pixel_ratio)
            .field("resize_max_side", &self.cfg.resize_max_side)
            .field("loaded", &self.raw.lock().is_some())
            .finish()
    }
}

impl TractDbnetDetector {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: OcrConfig, raw: Box<dyn RawDetector>) -> Self {
        Self {
            cfg,
            raw: Mutex::new(Some(raw)),
        }
    }

    // `coverage(off)`：真实模型加载路径——`load_runnable` 在 `_real.rs` 由
    // ignore-regex 排除；CI 不分发 ONNX 文件，ensure_raw 在 lib unit / 集成测试
    // 始终因为 `with_raw` 提前注入 `Some(...)` 而 early return。逻辑由
    // `dispatch_returns_invalid_input_when_model_path_empty` + 真模型手动验证
    // （plan「验证」第 5 步）覆盖。
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<()> {
        let mut guard = self.raw.lock();
        if guard.is_some() {
            return Ok(());
        }
        let model = load_runnable(Path::new(&self.cfg.det_model_path))?;
        *guard = Some(Box::new(TractRawDetector { model }));
        Ok(())
    }
}

impl TextDetector for TractDbnetDetector {
    fn has_text(&self, _path: &Utf8Path, image_bytes: &[u8]) -> io::Result<bool> {
        self.ensure_raw()?;
        let input = preprocess(image_bytes, self.cfg.resize_max_side)?;
        let output = {
            let guard = self.raw.lock();
            guard
                .as_ref()
                .expect("ensure_raw set Some before lock release")
                .run(input)?
        };
        Ok(decide(
            &output,
            self.cfg.binarize_threshold,
            self.cfg.min_text_pixel_ratio,
        ))
    }
}

/// 检测器装配入口：dispatch 调用此函数构造 `Box<dyn TextDetector>`。`det_model_path`
/// 为空时报 `InvalidInput`（feature on 但未配模型），让用户感知配置缺失。
///
/// # Errors
///
/// 当 `det_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_detector(cfg: &OcrConfig) -> io::Result<Box<dyn TextDetector>> {
    if cfg.det_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.ocr.det_model_path is empty; set TIDYMEDIA_OCR_DET_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractDbnetDetector {
        cfg: cfg.clone(),
        raw: Mutex::new(None),
    }))
}

/// 把图像字节解码、resize 到 32 倍数短边、ImageNet normalize、HWC→CHW、add batch
/// dim，返回 NCHW `[1, 3, H, W]` f32 Tensor。
pub(crate) fn preprocess(bytes: &[u8], max_side: u32) -> io::Result<Tensor> {
    // ImageNet 归一化：DBNet 训练默认 mean=[0.485,0.456,0.406] std=[0.229,0.224,0.225]
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];

    let img = image::load_from_memory(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")))?;
    // image crate 解码成功返非空 DynamicImage；零维度由 `target_size::max(32)`
    // 兜底（任何输入维度都被钳到 ≥ 32），防御性 zero 检查移除避免不可达分支。
    let (orig_w, orig_h) = img.dimensions();
    let (width, height) = target_size(orig_w, orig_h, max_side);
    let rgb = img
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_rgb8();

    let h_usize = height as usize;
    let w_usize = width as usize;
    let mut chw = vec![0.0_f32; 3 * h_usize * w_usize];
    let plane = h_usize * w_usize;
    for (idx, px) in rgb.pixels().enumerate() {
        let y = idx / w_usize;
        let x = idx % w_usize;
        for ch in 0..3 {
            let v = f32::from(px.0[ch]) / 255.0;
            chw[ch * plane + y * w_usize + x] = (v - MEAN[ch]) / STD[ch];
        }
    }
    // shape_vec 失败仅当 chw.len() != prod(shape)，由上方 vec 预分配大小保证不可能；
    // expect 标内部不变量，避免不可达 Err arm 拉低覆盖率。
    let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, h_usize, w_usize), chw)
        .expect("internal: chw vec sized exactly 1*3*H*W")
        .into_tensor();
    Ok(tensor)
}

/// 按 32 对齐 + 短边不超过 `max_side` 计算 resize 后宽高。DBNet 要求 H/W 是 32 倍数
/// （池化层 5 次 2× 下采样）。
fn target_size(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let scale = if w.max(h) > max_side {
        f64::from(max_side) / f64::from(w.max(h))
    } else {
        1.0
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scale ∈ (0, 1]，sw/sh 为图像维度乘积上限 ~10^4，截断和符号丢失不可达"
    )]
    let mut sw = (f64::from(w) * scale).round() as u32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "同上"
    )]
    let mut sh = (f64::from(h) * scale).round() as u32;
    sw = align32(sw);
    sh = align32(sh);
    (sw.max(32), sh.max(32))
}

fn align32(v: u32) -> u32 {
    v.div_ceil(32) * 32
}

/// sigmoid 输出图二值化后看前景像素占比是否超阈值。`output` 期望 NCHW `[1, 1, H, W]`
/// 但只看数值，不校验维度（DBNet 输出固定形状，由模型保证）。
///
/// `cast_to` / `as_slice` 仅当 Datum 类型不可转或非 contiguous 时失败——本调用
/// f32 → f32 + contiguous Array4 都不可能触发，用 `expect` 标内部不变量。
pub(crate) fn decide(output: &Tensor, binarize_threshold: f32, min_text_pixel_ratio: f32) -> bool {
    let cast = output
        .cast_to::<f32>()
        .expect("internal: f32→f32 cast must not fail");
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .expect("internal: contiguous Array4 always yields a slice");
    if slice.is_empty() {
        return false;
    }
    let hits = slice.iter().filter(|&&v| v > binarize_threshold).count();
    #[expect(
        clippy::cast_precision_loss,
        reason = "像素总数最多 ~10^6，f32 精度足以表达比例"
    )]
    let ratio = hits as f32 / slice.len() as f32;
    ratio > min_text_pixel_ratio
}

#[cfg(test)]
#[path = "tract_dbnet_tests.rs"]
mod tests;
