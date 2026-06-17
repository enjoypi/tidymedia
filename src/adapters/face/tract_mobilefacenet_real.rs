//! 真实 `MobileFaceNet` ONNX 加载。走 `--ignore-filename-regex='_real\.rs$'`
//! 排除整文件（CI 无模型不可触发）。与 `tract_dbnet_real.rs` 同套路。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_mobilefacenet::FacenetModel;

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
