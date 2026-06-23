//! tract-onnx 实现 `FaceMeshDetector`：跑 `MediaPipe` `FaceMesh` 输出 468 个 3D 关键点。
//!
//! 输入：192×192 RGB → `[0, 1]` 归一化 NCHW（`MediaPipe` `TFLite` 模型转 ONNX 后惯用）
//! 输出：`[1, 1404]` 或 `[1, 468, 3]`（视模型变体），统一 reshape 成 `Vec<[f32; 3]>` 468 项

use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_facemesh_real::load_runnable;
use crate::usecases::config::FaceConfig;
use crate::usecases::face::FaceMeshDetector;

pub(crate) type FaceMeshModel = Arc<TypedRunnableModel>;

const INPUT_SIDE: u32 = 192;
const MESH_POINTS: usize = 468;
const POINT_DIMS: usize = 3;

pub(crate) trait RawFaceMesh: Send + Sync {
    /// 接 NCHW `[1, 3, 192, 192]` f32；返 `468 * 3 = 1404` f32 总长（任意 reshape）。
    ///
    /// # Errors
    ///
    /// 模型推理失败或输出维度不符时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

struct TractRawFaceMesh {
    model: FaceMeshModel,
}

impl RawFaceMesh for TractRawFaceMesh {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract FaceMesh run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("tract FaceMesh returned no output tensor"))?;
        Ok(first.into_tensor())
    }
}

pub struct TractFaceMeshDetector {
    cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    raw: OnceLock<Box<dyn RawFaceMesh>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    load_lock: Mutex<()>,
}

impl std::fmt::Debug for TractFaceMeshDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractFaceMeshDetector")
            .field("facemesh_model_path", &self.cfg.facemesh_model_path)
            .field("loaded", &self.raw.get().is_some())
            .field("load_lock", &self.load_lock)
            .finish()
    }
}

impl TractFaceMeshDetector {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawFaceMesh>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(raw);
        Self {
            cfg,
            raw: cell,
            load_lock: Mutex::new(()),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<&dyn RawFaceMesh> {
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let _guard = self.load_lock.lock();
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let model = load_runnable(Path::new(&self.cfg.facemesh_model_path))?;
        let boxed: Box<dyn RawFaceMesh> = Box::new(TractRawFaceMesh { model });
        let _ = self.raw.set(boxed);
        Ok(self
            .raw
            .get()
            .expect("OnceLock set by self under load_lock")
            .as_ref())
    }
}

impl FaceMeshDetector for TractFaceMeshDetector {
    fn detect_mesh(
        &self,
        _path: &Utf8Path,
        face_crop: &image::RgbImage,
    ) -> io::Result<Vec<[f32; 3]>> {
        let raw = self.ensure_raw()?;
        let input = preprocess(face_crop)?;
        let output = raw.run(input)?;
        decode(&output)
    }
}

/// `facemesh_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `facemesh_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_facemesh(cfg: &FaceConfig) -> io::Result<Box<dyn FaceMeshDetector>> {
    if cfg.facemesh_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.facemesh_model_path is empty; set TIDYMEDIA_FACE_FACEMESH_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractFaceMeshDetector {
        cfg: cfg.clone(),
        raw: OnceLock::new(),
        load_lock: Mutex::new(()),
    }))
}

/// 输入 RGB → 192×192 → `[0, 1]` 归一化 NCHW `[1, 3, 192, 192]`。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，? 兼容未来动态 shape）。
pub(crate) fn preprocess(img: &image::RgbImage) -> io::Result<Tensor> {
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
    // 同 mobilefacenet.preprocess：const 形状下 Err arm 不可达，map_err 让 caller 经 ? 传播。
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .map_err(|e| io::Error::other(format!("facemesh preprocess shape: {e}")))
        .map(IntoTensor::into_tensor)
}

/// 取 468*3 = 1404 个 f32 → `Vec<[f32; 3]>` 468 项。
pub(crate) fn decode(output: &Tensor) -> io::Result<Vec<[f32; 3]>> {
    let cast = output
        .cast_to::<f32>()
        .map_err(|e| io::Error::other(format!("facemesh output not f32-castable: {e}")))?;
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .map_err(|e| io::Error::other(format!("facemesh output slice: {e}")))?;
    let expected = MESH_POINTS * POINT_DIMS;
    if slice.len() < expected {
        return Err(io::Error::other(format!(
            "facemesh output len {} < expected {expected}",
            slice.len()
        )));
    }
    let mut pts = Vec::with_capacity(MESH_POINTS);
    for i in 0..MESH_POINTS {
        let off = i * POINT_DIMS;
        pts.push([slice[off], slice[off + 1], slice[off + 2]]);
    }
    Ok(pts)
}

#[cfg(test)]
#[path = "tract_facemesh_tests.rs"]
mod tests;
