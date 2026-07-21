//! tract-onnx 实现的 [`DocumentClassifier`]：bge-small-zh embedding 做 zero-shot
//! 类目相似度分类。
//!
//! 设计（与 `adapters::ocr::tract_dbnet` 同套路）：
//! - **懒加载**：首次 `classify` 触发模型 + tokenizer 加载，随后对每个
//!   `categories[].description` 各 embed 一次缓存原型向量（`Mutex<Option<...>>`
//!   兼容 stable，加载 idempotent 竞态无副作用）
//! - **`RawEmbedder` 注入**：`embed(text) -> 未归一 CLS 向量`；tokenize/pad/
//!   forward 全在 `tract_embed_real.rs`（覆盖率 ignore-regex 排除），本文件只做
//!   normalize / cosine / argmax（纯函数，单测全分支）
//! - 阈值裁决归调用方（`usecases::copy::classify` 比 `score_min`）

use std::io;

use camino::Utf8Path;
use parking_lot::Mutex;

use super::tract_embed_real::load_raw_embedder;
use crate::usecases::classify::{Classification, DocumentClassifier};
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
struct LoadedState {
    raw: Box<dyn RawEmbedder>,
    prototypes: Vec<(String, Vec<f32>)>,
}

/// 分类器主体：持配置 + 懒加载状态。
pub struct TractEmbedClassifier {
    cfg: ClassifyConfig,
    state: Mutex<Option<LoadedState>>,
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

    // `coverage(off)`：真实模型加载路径——`load_raw_embedder` 在 `_real.rs` 由
    // ignore-regex 排除；CI 不分发 ONNX/tokenizer 文件，ensure_loaded 在 lib
    // unit 始终因 `with_raw` 提前注入 Some 而 early return。
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_loaded(&self) -> io::Result<()> {
        let mut guard = self.state.lock();
        if guard.is_some() {
            return Ok(());
        }
        let raw = load_raw_embedder(&self.cfg.embed_model_path, &self.cfg.tokenizer_path)?;
        let prototypes = build_prototypes(raw.as_ref(), &self.cfg)?;
        *guard = Some(LoadedState { raw, prototypes });
        Ok(())
    }
}

impl DocumentClassifier for TractEmbedClassifier {
    fn classify(&self, _path: &Utf8Path, text: &str) -> io::Result<Classification> {
        if self.cfg.categories.is_empty() {
            return Ok(empty_classification());
        }
        self.ensure_loaded()?;
        let vec = {
            let guard = self.state.lock();
            let state = guard
                .as_ref()
                .expect("ensure_loaded set Some before lock release");
            let v = state.raw.embed(text)?;
            best_match(&normalize(v), &state.prototypes)
        };
        Ok(vec)
    }
}

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

/// 类目列表为空时的占位结果：空串类目 + `-inf` 分数让任何 `score_min` 阈值
/// 都把它裁决为 uncategorized。
fn empty_classification() -> Classification {
    Classification {
        category: String::new(),
        score: f32::NEG_INFINITY,
    }
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

/// L2 归一化；零向量（模型异常输出）原样返回，后续 cosine 得 0 不 NaN。
pub(crate) fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 && norm.is_finite() {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// 归一化向量与全部原型做 cosine（点积），NaN 视 -inf 取 argmax。
pub(crate) fn best_match(v: &[f32], prototypes: &[(String, Vec<f32>)]) -> Classification {
    let best = prototypes
        .iter()
        .map(|(name, p)| (name, cosine(v, p)))
        .max_by(|(_, a), (_, b)| total_cmp_nan_as_neg_inf(*a, *b));
    match best {
        Some((name, score)) => Classification {
            category: name.clone(),
            score,
        },
        None => empty_classification(),
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// 与 `usecases::cull::crop::total_cmp_nan_as_neg_inf` 同语义的本地副本
// （原件 `pub(super)` 对 adapters 不可见；4 行 helper 不值得为此上提公共层）。
fn total_cmp_nan_as_neg_inf(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.total_cmp(&b),
    }
}

#[cfg(test)]
#[path = "tract_embed_tests.rs"]
mod tests;
