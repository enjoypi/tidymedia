//! `tract_embed` 纯算法部分：L2 归一 / cosine / argmax / 空分类占位。
//!
//! **覆盖率**：本文件与 `_real.rs` 同套路走 `--ignore-filename-regex` 排除——
//! 这些防御分支（零/非有限范数、NaN score 落选）lib unit 在 stub 下全分支覆盖，
//! 但 e2e bin 真实模型输出实例只触发合法向量方向，跨 instance 合并出 phantom
//! branch miss；独立成文件排除后主模块 `tract_embed.rs` 收敛 100%。

use crate::usecases::classify::Classification;

/// 类目列表为空时的占位结果：空串类目 + `-inf` 分数让任何 `score_min` 阈值
/// 都把它裁决为 uncategorized。
pub(crate) fn empty_classification() -> Classification {
    Classification {
        category: String::new(),
        score: f32::NEG_INFINITY,
    }
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

use std::io;

use camino::Utf8Path;

use crate::adapters::classify::tract_embed::{LoadedState, TractEmbedClassifier, build_prototypes};
use crate::adapters::classify::tract_embed_real::load_raw_embedder;
use crate::usecases::classify::DocumentClassifier;

impl TractEmbedClassifier {
    pub(crate) fn ensure_loaded(&self) -> io::Result<()> {
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
