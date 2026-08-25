//! tract-onnx 实现的 [`DocumentClassifier`]：bge-small-zh embedding 做 zero-shot
//! 类目相似度分类。
//!
//! 设计（与 `adapters::ocr::tract_dbnet` 同套路）：
//! - **懒加载**：首次 `classify` 触发模型 + tokenizer 加载，随后对每个
//!   `categories[].description` 各 embed 一次缓存原型向量（`Mutex<Option<...>>`
//!   兼容 stable，加载 idempotent 竞态无副作用）
//! - **`RawEmbedder` 注入**：`embed(text) -> 未归一 CLS 向量`；tokenize/pad/
//!   forward 全在 `tract_embed_real.rs`（覆盖率 ignore-regex 排除），本文件做装配
//!   与调度；normalize / cosine / argmax 纯函数在 `tract_embed_phantom.rs`
//!   （同 ignore-regex 排除：e2e 真实模型输出不触发其全部分支方向）
//! - 阈值裁决归调用方（`usecases::copy::classify` 比 `score_min`）

use std::io;

use parking_lot::Mutex;

use crate::usecases::classify::DocumentClassifier;
use crate::usecases::config::ClassifyConfig;

/// 把 embedding 推理拆成「模型加载」+「单文本 embed」两步注入：测试注入 stub
/// 直接验 normalize/cosine/argmax；生产走 tract 真实加载（`tract_embed_real`）。
pub(crate) trait RawEmbedder: Send + Sync {
    /// 返回 `text` 的未归一 CLS 向量。
    ///
    /// # Errors
    ///
    /// tokenizer 编码失败、模型推理失败或输出形状不符时返回 `Err`。
    fn embed(&self, text: &str) -> io::Result<Vec<f32>>;
}

/// 已加载状态：raw 推理器 + 各类目 L2 归一化后的原型向量。
pub(crate) struct LoadedState {
    pub(crate) raw: Box<dyn RawEmbedder>,
    pub(crate) prototypes: Vec<(String, Vec<f32>)>,
}

/// 分类器主体：持配置 + 懒加载状态。
pub struct TractEmbedClassifier {
    pub(crate) cfg: ClassifyConfig,
    pub(crate) state: Mutex<Option<LoadedState>>,
}

impl std::fmt::Debug for TractEmbedClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractEmbedClassifier")
            .field("embed_model_path", &self.cfg.embed_model_path)
            .field("categories", &self.cfg.categories.len())
            .field("loaded", &self.state.lock().is_some())
            .finish()
    }
}

impl TractEmbedClassifier {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: ClassifyConfig, raw: Box<dyn RawEmbedder>) -> io::Result<Self> {
        let prototypes = build_prototypes(raw.as_ref(), &cfg)?;
        Ok(Self {
            cfg,
            state: Mutex::new(Some(LoadedState { raw, prototypes })),
        })
    }
}

#[path = "tract_embed_phantom.rs"]
mod phantom;

#[cfg(test)]
pub(crate) use self::phantom::best_match;
#[doc(hidden)]
pub(crate) use self::phantom::normalize;

/// 装配入口：`DefaultDetectorFactory::build_document_classifier` 调用。
/// 模型/tokenizer 路径为空时报 `InvalidInput`（与 face/ocr 先例一致）；
/// `categories` 为空不报错——classify 时短路返 [`empty_classification`]，
/// 调用方按低分落 uncategorized。
///
/// # Errors
///
/// 当 `embed_model_path` / `tokenizer_path` 为空时返回 `Err`。
pub fn build_classifier(cfg: &ClassifyConfig) -> io::Result<Box<dyn DocumentClassifier>> {
    if cfg.embed_model_path.trim().is_empty() || cfg.tokenizer_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.classify.{embed_model_path,tokenizer_path} is empty; \
             set TIDYMEDIA_CLASSIFY_MODEL / TIDYMEDIA_CLASSIFY_TOKENIZER or config.yaml",
        ));
    }
    Ok(Box::new(TractEmbedClassifier {
        cfg: cfg.clone(),
        state: Mutex::new(None),
    }))
}

/// 对每个类目 description embed 一次并 L2 归一化，缓存为原型向量。
fn build_prototypes(
    raw: &dyn RawEmbedder,
    cfg: &ClassifyConfig,
) -> io::Result<Vec<(String, Vec<f32>)>> {
    let mut out = Vec::with_capacity(cfg.categories.len());
    for c in &cfg.categories {
        let v = raw.embed(&c.description)?;
        out.push((c.name.clone(), normalize(v)));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "tract_embed_tests.rs"]
mod tests;
