//! tract-onnx 实现 `FaceMeshDetector`：跑 `MediaPipe` `FaceMesh` 输出 468 个 3D 关键点。
//!
//! 输入：192×192 RGB → `[0, 1]` 归一化 NCHW（`MediaPipe` `TFLite` 模型转 ONNX 后惯用）
//! 输出：`[1, 1404]` 或 `[1, 468, 3]`（视模型变体），统一 reshape 成 `Vec<[f32; 3]>` 468 项

use std::io;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use crate::usecases::config::FaceConfig;
use crate::usecases::face::FaceMeshDetector;

pub(crate) type FaceMeshModel = Arc<TypedRunnableModel>;

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

pub struct TractFaceMeshDetector {
    pub(crate) cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    pub(crate) raw: OnceLock<Box<dyn RawFaceMesh>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    pub(crate) load_lock: Mutex<()>,
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
}

impl FaceMeshDetector for TractFaceMeshDetector {
    fn detect_mesh(
        &self,
        _path: &Utf8Path,
        face_crop: &image::RgbImage,
    ) -> io::Result<Vec<[f32; 3]>> {
        let raw = self.ensure_raw()?;
        let input = preprocess(face_crop);
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

// 输入预处理（192×192 resize + 归一化）独立到 `tract_facemesh_phantom.rs`：
// e2e bin 真实模型输出实例（crop 对齐 192）只触发 Cow::Borrowed 方向，跨 instance
// 合并出 phantom branch miss，独立成文件后走 ignore-regex 排除。
#[path = "tract_facemesh_phantom.rs"]
mod phantom;

#[doc(hidden)]
pub(crate) use self::phantom::preprocess;

/// 取 468*3 = 1404 个 f32 → `Vec<[f32; 3]>` 468 项。
#[doc(hidden)]
pub fn decode(output: &Tensor) -> io::Result<Vec<[f32; 3]>> {
    let cast = output.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .expect("cast 成功后 view 恒 contiguous");
    let expected = MESH_POINTS * POINT_DIMS;
    if slice.len() != expected {
        // 严格匹配 468*3 = 1404，不用 `<`：误配 face_mesh 变体（attention refinement
        // 版顶点顺序不同）时 len > 1404 若沿用 `<` 检查会通过而取前 468 得非 468
        // 面部点集致 EAR/嘴部索引错位。对齐 MobileFaceNet EMBED_DIM 严格 `!=` 套路
        // （CLAUDE.md「face embedding decode `slice.len()` MUST `!=` 严格匹配」）。
        return Err(io::Error::other(format!(
            "facemesh output len {} != expected {expected}; \
             check facemesh_model_path: must be 468*3=1404 static-shape mesh model, \
             not attention refinement variant",
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
