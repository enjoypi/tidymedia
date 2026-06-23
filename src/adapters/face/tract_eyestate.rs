//! tract-onnx 实现 `EyeStateClassifier`：跑 `YOLOv8` 眼态检测取闭眼最大置信。
//!
//! 模型源：`MichalMlodawski/open-closed-eye-detection`（`YOLOv8` 检测头，非 softmax）。
//! 输入：任意 RGB → 640×640 letterbox（灰底）→ `[0, 1]` 归一化 → NCHW `[1, 3, 640, 640]`
//! 输出：`[1, 6, 8400]` = 4 box `(cx,cy,w,h)` + 2 class conf `(open=0, closed=1)`；
//! 本实现遍历 8400 个 anchor 取 closed conf 最大值作为 blink probability。
//!
//! 注：closed 类索引按 README 描述（open=0, closed=1）；Netron 校对若反向需翻转
//! `CLOSED_CLASS_IDX`。
//!
//! `EyeStateClassifier` trait 契约（接 `eye_crop`）保留：把 eye crop 当全图送 letterbox
//! 后仍可被检测到 + 分类——比起经典 `MobileNetV3` softmax 二分类，`YOLOv8` 输出对
//! 输入尺度更鲁棒。

use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_eyestate_real::load_runnable;
use crate::usecases::config::FaceConfig;
use crate::usecases::face::EyeStateClassifier;

pub(crate) type EyeStateModel = Arc<TypedRunnableModel>;

const INPUT_SIDE: u32 = 640;
const NUM_CLASSES: usize = 2;
const BOX_DIMS: usize = 4;
const OUTPUT_CHANNELS: usize = BOX_DIMS + NUM_CLASSES;
const CLOSED_CLASS_IDX: usize = 1;

pub(crate) trait RawEyeState: Send + Sync {
    /// 接 NCHW `[1, 3, 640, 640]` f32；返 `[1, 6, anchors]` f32 `YOLOv8` 检测头输出。
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
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    raw: OnceLock<Box<dyn RawEyeState>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    load_lock: Mutex<()>,
}

impl std::fmt::Debug for TractEyeStateClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractEyeStateClassifier")
            .field("eyestate_model_path", &self.cfg.eyestate_model_path)
            .field("loaded", &self.raw.get().is_some())
            .field("load_lock", &self.load_lock)
            .finish()
    }
}

impl TractEyeStateClassifier {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawEyeState>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(raw);
        Self {
            cfg,
            raw: cell,
            load_lock: Mutex::new(()),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<&dyn RawEyeState> {
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let _guard = self.load_lock.lock();
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let model = load_runnable(Path::new(&self.cfg.eyestate_model_path))?;
        let boxed: Box<dyn RawEyeState> = Box::new(TractRawEyeState { model });
        let _ = self.raw.set(boxed);
        Ok(self
            .raw
            .get()
            .expect("OnceLock set by self under load_lock")
            .as_ref())
    }
}

impl EyeStateClassifier for TractEyeStateClassifier {
    fn classify_eye(&self, _path: &Utf8Path, eye_crop: &image::RgbImage) -> io::Result<f32> {
        let raw = self.ensure_raw()?;
        let input = preprocess(eye_crop)?;
        let output = raw.run(input)?;
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
        raw: OnceLock::new(),
        load_lock: Mutex::new(()),
    }))
}

/// 任意 RGB → 640×640 letterbox（灰底 114）→ `[0, 1]` 归一化 NCHW `[1, 3, 640, 640]`。
///
/// letterbox：保持长边按比例 resize 到 640，短边居中 padding 114（`YOLOv8` 默认填充值）。
/// 0 尺寸入参降级为全 padding 画布。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，? 兼容未来动态 shape）。
pub(crate) fn preprocess(img: &image::RgbImage) -> io::Result<Tensor> {
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
    // 同 mobilefacenet.preprocess：const 形状下 Err arm 不可达，map_err 让 caller 经 ? 传播。
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .map_err(|e| io::Error::other(format!("eyestate preprocess shape: {e}")))
        .map(IntoTensor::into_tensor)
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
pub(crate) fn decode(output: &Tensor) -> io::Result<f32> {
    let cast = output
        .cast_to::<f32>()
        .map_err(|e| io::Error::other(format!("eyestate output not f32-castable: {e}")))?;
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
        .map_err(|e| io::Error::other(format!("eyestate output slice: {e}")))?;
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

#[cfg(test)]
#[path = "tract_eyestate_tests.rs"]
mod tests;
