//! 真实 `MediaPipe` `FaceMesh` ONNX 加载。走 `--ignore-filename-regex='_real\.rs$'` 排除。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_facemesh::FaceMeshModel;

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
