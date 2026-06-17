//! tract-onnx 实现 `EyeStateClassifier`：跑 `MobileNetV3`-`EyeState` 二分类
//! （open vs closed）取闭眼概率。
//!
//! 输入：任意尺寸眼部 crop → 内部 resize 到 64×64 → `[0, 1]` 归一化 → NCHW
//! 输出：`[1, 2]` logits → softmax → 取 index 1（closed）概率
//!
//! 注：不同 `EyeState` 模型 close 索引可能为 0 或 1；本实现按 `InsightFace` 训练惯例
//! 取 index 1。若用户备的是 open-index=1 的模型，e2e 真跑时会得相反语义——
//! plan F 节风险 #1 已明示需 Netron 工具验证 ONNX 输出 layout。

use std::io;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::EyeStateClassifier;
use super::tract_eyestate_real::load_runnable;
use crate::usecases::config::FaceConfig;

pub(crate) type EyeStateModel = Arc<TypedRunnableModel>;

const INPUT_SIDE: u32 = 64;
const CLOSED_INDEX: usize = 1;
const NUM_CLASSES: usize = 2;

pub(crate) trait RawEyeState: Send + Sync {
    /// 接 NCHW `[1, 3, 64, 64]` f32；返 `[1, 2]` f32 logits。
    ///
    /// # Errors
    ///
    /// 模型推理失败或输出维度不符时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

struct TractRawEyeState {
    model: EyeStateModel,
}

impl RawEyeState for TractRawEyeState {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract EyeState run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("tract EyeState returned no output tensor"))?;
        Ok(first.into_tensor())
    }
}

pub struct TractEyeStateClassifier {
    cfg: FaceConfig,
    raw: Mutex<Option<Box<dyn RawEyeState>>>,
}

impl std::fmt::Debug for TractEyeStateClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractEyeStateClassifier")
            .field("eyestate_model_path", &self.cfg.eyestate_model_path)
            .field("loaded", &self.raw.lock().is_some())
            .finish()
    }
}

impl TractEyeStateClassifier {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawEyeState>) -> Self {
        Self {
            cfg,
            raw: Mutex::new(Some(raw)),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<()> {
        let mut guard = self.raw.lock();
        if guard.is_some() {
            return Ok(());
        }
        let model = load_runnable(Path::new(&self.cfg.eyestate_model_path))?;
        *guard = Some(Box::new(TractRawEyeState { model }));
        Ok(())
    }
}

impl EyeStateClassifier for TractEyeStateClassifier {
    fn classify_eye(&self, _path: &Utf8Path, eye_crop: &image::RgbImage) -> io::Result<f32> {
        self.ensure_raw()?;
        let input = preprocess(eye_crop);
        let output = {
            let guard = self.raw.lock();
            guard
                .as_ref()
                .expect("ensure_raw set Some before lock release")
                .run(input)?
        };
        decode(&output)
    }
}

/// `eyestate_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `eyestate_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_eyestate_classifier(cfg: &FaceConfig) -> io::Result<Box<dyn EyeStateClassifier>> {
    if cfg.eyestate_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.eyestate_model_path is empty; set TIDYMEDIA_FACE_EYESTATE_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractEyeStateClassifier {
        cfg: cfg.clone(),
        raw: Mutex::new(None),
    }))
}

/// 输入任意 RGB → 64×64 RGB → `[0, 1]` 归一化 NCHW `[1, 3, 64, 64]`。
pub(crate) fn preprocess(img: &image::RgbImage) -> Tensor {
    let resized = if img.width() == INPUT_SIDE && img.height() == INPUT_SIDE {
        img.clone()
    } else {
        image::imageops::resize(
            img,
            INPUT_SIDE,
            INPUT_SIDE,
            image::imageops::FilterType::Triangle,
        )
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
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .expect("internal: chw vec sized exactly 1*3*64*64")
        .into_tensor()
}

/// 取 `[1, 2]` logits → softmax → 闭眼 prob（index 1）。
pub(crate) fn decode(output: &Tensor) -> io::Result<f32> {
    let cast = output
        .cast_to::<f32>()
        .map_err(|e| io::Error::other(format!("eyestate output not f32-castable: {e}")))?;
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .map_err(|e| io::Error::other(format!("eyestate output slice: {e}")))?;
    if slice.len() < NUM_CLASSES {
        return Err(io::Error::other(format!(
            "eyestate output len {} < expected {NUM_CLASSES}",
            slice.len()
        )));
    }
    let logits = &slice[..NUM_CLASSES];
    // 数值稳定 softmax：减最大值
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: [f32; NUM_CLASSES] = [(logits[0] - max).exp(), (logits[1] - max).exp()];
    let sum = exps[0] + exps[1];
    if sum <= f32::EPSILON {
        return Err(io::Error::other("eyestate softmax sum underflow"));
    }
    Ok(exps[CLOSED_INDEX] / sum)
}

#[cfg(test)]
#[path = "tract_eyestate_tests.rs"]
mod tests;
