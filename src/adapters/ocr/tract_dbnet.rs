use std::io;

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_dbnet_real::{as_f32_vec, decode_rgb, resize_rgb, rgb_chw_tensor};
use crate::usecases::config::OcrConfig;
use crate::usecases::ocr::TextDetector;

pub(crate) trait RawDetector: Send + Sync {
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

pub struct TractDbnetDetector {
    pub(crate) cfg: OcrConfig,
    pub(crate) raw: Mutex<Option<Box<dyn RawDetector>>>,
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

#[expect(clippy::missing_errors_doc, reason = "配置错误经统一 io::Result 传播")]
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

pub(crate) fn preprocess(bytes: &[u8], max_side: u32) -> io::Result<Tensor> {
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];

    let rgb = decode_rgb(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")))?;
    let (width, height) = target_size(rgb.width(), rgb.height(), max_side);
    let rgb = resize_rgb(&rgb, width, height);
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
    Ok(rgb_chw_tensor(chw, h_usize, w_usize))
}

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

pub(crate) fn decide(output: &Tensor, binarize_threshold: f32, min_text_pixel_ratio: f32) -> bool {
    if output.len() == 0 {
        return false;
    }
    let values = as_f32_vec(output);
    let hits = values.iter().filter(|&&v| v > binarize_threshold).count();
    #[expect(
        clippy::cast_precision_loss,
        reason = "像素总数最多 ~10^6，f32 精度足以表达比例"
    )]
    let ratio = hits as f32 / values.len() as f32;
    ratio > min_text_pixel_ratio
}

#[cfg(test)]
#[path = "tract_dbnet_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "tract_dbnet_tests.rs"]
mod tests;
