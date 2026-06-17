//! 文本检测 Adapter：[`TextDetector`](crate::usecases::ocr::TextDetector) 的具体实现 + Fake。
//!
//! Port 定义见 `crate::usecases::ocr`。真实实现走 tract-onnx + `PaddleOCR` `DBNet`，
//! `tract_dbnet_real.rs` 走 `--ignore-filename-regex` 排除（CI 无 ONNX 模型不可触发）。

pub mod fake;
pub mod tract_dbnet;
pub mod tract_dbnet_real;

pub use tract_dbnet::build_detector;
