//! 真实 `PaddleOCR` `DBNet` `det.onnx` 加载（tract-onnx 默认编译，需用户备真模型文件）。
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

/// 读 ONNX 文件 → `TypedModel`（不调 `into_optimized`）→ `Arc<RunnableModel>`。
///
/// **不 optimize**：`PaddleOCR` `DBNet` `det.onnx` 是 dynamic H/W 输入（`[1, 3, ?, ?]`），
/// tract `into_optimized()` 对未固化的 symbolic dim 做不了形状传播，加载即失败；
/// 4 个 face 模型经 `scripts/simplify_onnx.py` 化简到静态 shape 后才能 optimize，
/// `DBNet` 不在该脚本范围内（用户自备 onnx）。`into_typed` 仅做类型推导不需要静态 shape，
/// `into_runnable` 接受 typed model 直接出 `RunnableModel`。
///
/// # Errors
///
/// 文件不存在、ONNX 解析失败、类型推导失败或 runnable 装配失败时返回 `Err`。
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
