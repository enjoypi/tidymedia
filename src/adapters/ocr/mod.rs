//! 文本检测 Gateway 抽象：把「这张图含不含可识别文字」的判定封进单一 trait，
//! 让 usecase 不直接依赖 OCR 库。具体实现位于本目录下 `tract_dbnet*.rs`（feature
//! `ocr-detect` 启用）；测试用 `FakeTextDetector`（路径查表 + Err 注入）。
//!
//! 设计要点：
//! - trait 对象安全（单方法、`&[u8]` + `&Utf8Path` 入参、无泛型）
//! - 返回 `io::Result<bool>` 而非 `Result<TextRegions>`：本子命令仅需「有/无」二元判定，
//!   不取 polygon 坐标——让后处理可跳过 connected-components / contour 复杂度
//! - `path` 入参是 detector 的「decision context」：真实 tract 实现忽略它，
//!   fake 实现用它做路径查表 / Err 注入，便于 e2e 写「这张图含文本，那张不含」类断言
//! - `Send + Sync + Debug` 与项目 `Box<dyn MediaReader>` 约定一致，支持后续 rayon

use std::io;

use camino::Utf8Path;

pub mod fake;

#[cfg(feature = "ocr-detect")]
pub mod tract_dbnet;
#[cfg(feature = "ocr-detect")]
pub mod tract_dbnet_real;

#[cfg(feature = "ocr-detect")]
pub use tract_dbnet::build_detector;

/// 文本检测 Gateway。实现者按 path + 字节判定，**不持** Backend
/// （Clean Architecture：URI/backend 解析在外层）。
pub trait TextDetector: Send + Sync + std::fmt::Debug {
    /// 判定 `image_bytes` 解码后的图像是否含文本。`path` 仅作为调用上下文（日志键、
    /// fake 注入键），实现者解码字节进行真实推理。
    ///
    /// # Errors
    ///
    /// 当字节解码失败、模型推理失败或路径级注入错误时返回 `Err`。
    fn has_text(&self, path: &Utf8Path, image_bytes: &[u8]) -> io::Result<bool>;
}
