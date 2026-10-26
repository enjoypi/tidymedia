use std::io;
use std::path::Path;
use std::sync::Arc;

use tract_onnx::prelude::*;

use super::tract_dbnet::{RawDetector, TractDbnetDetector};

pub(crate) type DetModel = Arc<TypedRunnableModel>;

pub(crate) struct TractRawDetector {
    pub(crate) model: DetModel,
}

impl RawDetector for TractRawDetector {
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract DBNet run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .expect("模型 run 成功必有输出 tensor");
        Ok(first.into_tensor())
    }
}

pub(crate) fn decode_rgb(bytes: &[u8]) -> Result<image::RgbImage, image::ImageError> {
    Ok(image::load_from_memory(bytes)?.to_rgb8())
}

pub(crate) fn resize_rgb(img: &image::RgbImage, width: u32, height: u32) -> image::RgbImage {
    image::imageops::resize(img, width, height, image::imageops::FilterType::Triangle)
}

pub(crate) fn rgb_chw_tensor(chw: Vec<f32>, h: usize, w: usize) -> Tensor {
    tract_ndarray::Array4::from_shape_vec((1, 3, h, w), chw)
        .expect("internal: chw vec sized exactly 1*3*H*W")
        .into_tensor()
}

pub(crate) fn as_f32_vec(t: &Tensor) -> Vec<f32> {
    let cast = t.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
    cast.view()
        .as_slice::<f32>()
        .expect("cast 成功后 view 恒 contiguous")
        .to_vec()
}

pub(crate) fn load_runnable(path: &Path) -> io::Result<DetModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load DBNet ONNX {}: {e}", path.display())))?
        .into_typed()
        .map_err(|e| io::Error::other(format!("type DBNet model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make DBNet runnable: {e}")))?;
    Ok(model)
}

pub(crate) fn load_detector(path: &Path) -> io::Result<Box<dyn RawDetector>> {
    let model = load_runnable(path)?;
    Ok(Box::new(TractRawDetector { model }))
}

impl TractDbnetDetector {
    pub(crate) fn ensure_raw(&self) -> io::Result<()> {
        self.ensure_raw_with(load_detector)
    }

    pub(crate) fn ensure_raw_with(
        &self,
        loader: fn(&Path) -> io::Result<Box<dyn RawDetector>>,
    ) -> io::Result<()> {
        let mut guard = self.raw.lock();
        if guard.is_some() {
            return Ok(());
        }
        let raw = loader(Path::new(&self.cfg.det_model_path))?;
        *guard = Some(raw);
        Ok(())
    }
}
