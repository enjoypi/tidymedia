//! 真实 `MobileFaceNet` ONNX 加载。走 `--ignore-filename-regex='_real\.rs$'`
//! 排除整文件（CI 无模型不可触发）。与 `tract_dbnet_real.rs` 同套路。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_mobilefacenet::{FacenetModel, RawFacenet, TractFacenetEmbedder};

pub(crate) struct TractRawFacenet {
    pub(crate) model: FacenetModel,
}

impl RawFacenet for TractRawFacenet {
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract MobileFaceNet run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .expect("模型 run 成功必有输出 tensor");
        Ok(first.into_tensor())
    }
}

/// 读 ONNX → optimized → runnable，失败统一映射 `io::Error::Other`。
///
/// # Errors
///
/// 文件不存在、ONNX 解析、优化或形状推导失败时返回 `Err`。
pub(crate) fn load_runnable(path: &Path) -> io::Result<FacenetModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load MobileFaceNet ONNX {}: {e}", path.display())))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize MobileFaceNet model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make MobileFaceNet runnable: {e}")))?;
    Ok(model)
}

impl TractFacenetEmbedder {
    pub(crate) fn ensure_raw(&self) -> io::Result<&dyn RawFacenet> {
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let _guard = self.load_lock.lock();
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let model = load_runnable(Path::new(&self.cfg.facenet_model_path))?;
        let boxed: Box<dyn RawFacenet> = Box::new(TractRawFacenet { model });
        let _ = self.raw.set(boxed);
        Ok(self
            .raw
            .get()
            .expect("OnceLock set by self under load_lock")
            .as_ref())
    }
}
