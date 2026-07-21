//! 文档内容分类 Output Port：把「这段文本最接近哪个用户类目」判定封进单一 trait，
//! 让 `copy-doc`/`move-doc` use case 不知道具体 embedding 模型。
//!
//! 设计要点（与 [`crate::usecases::ocr::TextDetector`] 同哲学）：
//! - trait 对象安全（单方法、`&str` + `&Utf8Path` 入参、无泛型）
//! - 返回最相近类目 + cosine 相似度；**阈值裁决归调用方**（`score_min` 是
//!   use case 配置，分类器只报「最像谁、多像」）
//! - `path` 入参是 decision context：真实 tract 实现忽略它，fake 实现用它做
//!   Err 注入键，便于 e2e 写「这份文档分类失败」类断言
//! - 命名避开 `TextClassifier`——项目已有 `TextDetector`（OCR 判「有没有文字」），
//!   `DocumentClassifier` 消歧
//! - **位置**：推理类 Output Port 定义在 use case 层（CLAUDE.md「新增 Output
//!   Port trait MUST 落到内层」）；具体实现（`tract_embed*.rs` + Fake）在
//!   `adapters::classify`

use std::io;

use camino::Utf8Path;

/// 单次分类结果：最相近类目名 + cosine 相似度。
#[derive(Clone, Debug)]
pub struct Classification {
    /// 命中的类目名（`config.backend.classify.categories[].name`）。
    /// 类目列表为空时为空串（调用方按 `score` 低于阈值落 uncategorized）。
    pub category: String,
    /// 与命中类目原型向量的 cosine 相似度；类目列表为空时为 `f32::NEG_INFINITY`。
    pub score: f32,
}

/// 文档内容分类 Gateway。实现者按文本做 zero-shot embedding 相似度判定，
/// **不持** Backend（文本提取在外层完成）。
pub trait DocumentClassifier: Send + Sync + std::fmt::Debug {
    /// 判定 `text` 最接近的用户类目。`path` 仅作调用上下文（日志键、fake 注入键）。
    ///
    /// # Errors
    ///
    /// 当模型/tokenizer 加载失败、推理失败或路径级注入错误时返回 `Err`。
    fn classify(&self, path: &Utf8Path, text: &str) -> io::Result<Classification>;
}
