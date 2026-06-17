//! tract-onnx 实现 `FaceDetector`：跑 SCRFD-500M-bn-kps 输出人脸 bbox + 5 点关键点。
//!
//! 设计：
//! - preprocess letterbox 到 640×640（保持长宽比 + 灰色 padding）；scale + padding
//!   meta 存 `Mutex<Option<ScaleMeta>>` 让 postprocess 把坐标逆映射回原图
//! - 真实 model.run + 三 stride anchor 解码 + NMS 在 `tract_scrfd_real.rs`
//!   （ignore-regex 排除整文件，CI 无 ONNX 模型不可触发）
//! - 单元测试用 `Vec<FaceDetection>` 直返的 stub `RawScrfd` 验装配 + preprocess

use std::io;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_scrfd_real::{ScaleMeta, TractRawScrfd, load_runnable};
use super::{FaceDetection, FaceDetector};
use crate::usecases::config::FaceConfig;

pub(crate) type ScrfdModel = Arc<TypedRunnableModel>;

pub(crate) const INPUT_SIDE: u32 = 640;
const SCORE_THRESHOLD: f32 = 0.5;
const NMS_IOU: f32 = 0.4;

pub(crate) trait RawScrfd: Send + Sync {
    /// 接预处理 NCHW `[1, 3, 640, 640]` f32；返已解码 + NMS 的人脸列表。
    /// trait 一步返结果（不暴露 raw tensor），让 stub 简单，anchor 算法集中真实路径。
    ///
    /// # Errors
    ///
    /// 模型推理失败、输出维度异常或 anchor 解码失败时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Vec<FaceDetection>>;
}

pub struct TractScrfdDetector {
    cfg: FaceConfig,
    raw: Mutex<Option<Box<dyn RawScrfd>>>,
    scale_meta: Arc<Mutex<Option<ScaleMeta>>>,
}

impl std::fmt::Debug for TractScrfdDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractScrfdDetector")
            .field("scrfd_model_path", &self.cfg.scrfd_model_path)
            .field("loaded", &self.raw.lock().is_some())
            .field("scale_meta_set", &self.scale_meta.lock().is_some())
            .finish()
    }
}

impl TractScrfdDetector {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawScrfd>) -> Self {
        Self {
            cfg,
            raw: Mutex::new(Some(raw)),
            scale_meta: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<()> {
        let mut guard = self.raw.lock();
        if guard.is_some() {
            return Ok(());
        }
        let model = load_runnable(Path::new(&self.cfg.scrfd_model_path))?;
        *guard = Some(Box::new(TractRawScrfd {
            model,
            score_threshold: SCORE_THRESHOLD,
            nms_iou: NMS_IOU,
            scale_meta: self.scale_meta.clone(),
        }));
        Ok(())
    }
}

impl FaceDetector for TractScrfdDetector {
    fn detect_faces(&self, _path: &Utf8Path, image_bytes: &[u8]) -> io::Result<Vec<FaceDetection>> {
        self.ensure_raw()?;
        let (input, meta) = preprocess(image_bytes)?;
        *self.scale_meta.lock() = Some(meta);
        let detections = {
            let guard = self.raw.lock();
            guard
                .as_ref()
                .expect("ensure_raw set Some before lock release")
                .run(input)?
        };
        Ok(detections)
    }
}

/// `scrfd_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `scrfd_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_scrfd_detector(cfg: &FaceConfig) -> io::Result<Box<dyn FaceDetector>> {
    if cfg.scrfd_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.scrfd_model_path is empty; set TIDYMEDIA_FACE_SCRFD_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractScrfdDetector {
        cfg: cfg.clone(),
        raw: Mutex::new(None),
        scale_meta: Arc::new(Mutex::new(None)),
    }))
}

/// Letterbox 把图像 resize 到 640×640（保持长宽比，灰色 128 padding），同时记录
/// scale + padding 给 postprocess 逆映射用。返 `(NCHW tensor, ScaleMeta)`。
pub(crate) fn preprocess(bytes: &[u8]) -> io::Result<(Tensor, ScaleMeta)> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")))?;
    let rgb = img.to_rgb8();
    let (orig_w, orig_h) = (rgb.width(), rgb.height());
    let side_f = f32::from(u16::try_from(INPUT_SIDE).expect("640 fits u16"));
    #[expect(clippy::cast_precision_loss, reason = "图像维度 ≤ 65535，f32 精度够")]
    let scale = (side_f / orig_w as f32).min(side_f / orig_h as f32);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scale ∈ (0, 1]，乘原维度后截断到 u32 安全"
    )]
    let new_w = (orig_w as f32 * scale).round() as u32;
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "同上"
    )]
    let new_h = (orig_h as f32 * scale).round() as u32;
    let resized = image::imageops::resize(
        &rgb,
        new_w.max(1),
        new_h.max(1),
        image::imageops::FilterType::Triangle,
    );
    let pad_x = (INPUT_SIDE - new_w.min(INPUT_SIDE)) / 2;
    let pad_y = (INPUT_SIDE - new_h.min(INPUT_SIDE)) / 2;
    let mut canvas =
        image::RgbImage::from_pixel(INPUT_SIDE, INPUT_SIDE, image::Rgb([128, 128, 128]));
    image::imageops::overlay(&mut canvas, &resized, i64::from(pad_x), i64::from(pad_y));

    let side = INPUT_SIDE as usize;
    let plane = side * side;
    let mut chw = vec![0.0_f32; 3 * plane];
    for (idx, px) in canvas.pixels().enumerate() {
        let y = idx / side;
        let x = idx % side;
        for ch in 0..3 {
            // SCRFD 训练: (v - 127.5) / 128 → 近 [-1, 1]
            chw[ch * plane + y * side + x] = (f32::from(px.0[ch]) - 127.5) / 128.0;
        }
    }
    let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .expect("internal: chw vec sized exactly 1*3*640*640")
        .into_tensor();
    #[expect(clippy::cast_precision_loss, reason = "pad ≤ 640，f32 精确")]
    let meta = ScaleMeta {
        scale,
        pad_x: pad_x as f32,
        pad_y: pad_y as f32,
    };
    Ok((tensor, meta))
}

#[cfg(test)]
#[path = "tract_scrfd_tests.rs"]
mod tests;
