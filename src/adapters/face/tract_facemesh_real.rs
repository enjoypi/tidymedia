//! 真实 `MediaPipe` `FaceMesh` ONNX 加载。走 `--ignore-filename-regex='_real\.rs$'` 排除。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_facemesh::{FaceMeshModel, RawFaceMesh, TractFaceMeshDetector};

pub(crate) struct TractRawFaceMesh {
    pub(crate) model: FaceMeshModel,
}

impl RawFaceMesh for TractRawFaceMesh {
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract FaceMesh run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .expect("模型 run 成功必有输出 tensor");
        Ok(first.into_tensor())
    }
}

/// 读 ONNX → optimized → runnable。
///
/// # Errors
///
/// 文件不存在、ONNX 解析、优化或形状推导失败时返回 `Err`。
pub(crate) fn load_runnable(path: &Path) -> io::Result<FaceMeshModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load FaceMesh ONNX {}: {e}", path.display())))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize FaceMesh model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make FaceMesh runnable: {e}")))?;
    Ok(model)
}

impl TractFaceMeshDetector {
    pub(crate) fn ensure_raw(&self) -> io::Result<&dyn RawFaceMesh> {
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
