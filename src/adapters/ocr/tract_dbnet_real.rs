//! 真实 `PaddleOCR` `DBNet` `det.onnx` 加载（feature `ocr-detect` + 真模型文件）。
//!
//! **覆盖率**：本文件走 `--ignore-filename-regex='_real\.rs$'` 排除整文件——
//! 加载真实 ONNX 文件在 CI 不可触发（模型不入 git），fake 测试在 `tract_dbnet::tests`
//! 通过 `RawDetector` trait 注入 stub model 覆盖前/后处理。
//!
//! 文件名 `_real.rs` 是项目约定，与 `*_real.rs` SMB/ADB/MTP 真实 client 同套路。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_dbnet::DetModel;

/// 读 ONNX 文件 → `InferenceModel` → optimized `TypedModel` → `Arc<RunnableModel>`
/// （`into_runnable()` 本身已 Arc 包装）。失败统一映射为 `io::Error::Other` 让 dispatch
/// 层透传到 CLI 错误退出码。
///
/// # Errors
///
/// 文件不存在、ONNX 解析失败、模型优化失败或形状推导失败时返回 `Err`。
pub(crate) fn load_runnable(path: &Path) -> io::Result<DetModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load DBNet ONNX {}: {e}", path.display())))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize DBNet model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make DBNet runnable: {e}")))?;
    Ok(model)
}
