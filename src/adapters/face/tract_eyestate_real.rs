//! 真实 `EyeState` ONNX 加载。走 `--ignore-filename-regex='_real\.rs$'` 排除整文件。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_eyestate::{EyeStateModel, RawEyeState, TractEyeStateClassifier};

/// 真实模型适配：`load_runnable` 成功后才构造；`run` 的 `model.run` 失败闭包
/// 在正常推理恒不触发，故随 `_real.rs` 一起被 ignore-regex 排除。
pub(crate) struct TractRawEyeState {
    pub(crate) model: EyeStateModel,
}

impl RawEyeState for TractRawEyeState {
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract EyeState run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .expect("模型 run 成功必有输出 tensor");
        Ok(first.into_tensor())
    }
}

impl TractEyeStateClassifier {
    pub(crate) fn ensure_raw(&self) -> io::Result<&dyn RawEyeState> {
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

/// 读 ONNX → optimized → runnable。
///
/// # Errors
///
/// 文件不存在、ONNX 解析、优化或形状推导失败时返回 `Err`。
pub(crate) fn load_runnable(path: &Path) -> io::Result<EyeStateModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load EyeState ONNX {}: {e}", path.display())))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize EyeState model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make EyeState runnable: {e}")))?;
    Ok(model)
}
