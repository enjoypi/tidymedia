use std::io;

use parking_lot::Mutex;

use crate::usecases::classify::DocumentClassifier;
use crate::usecases::config::ClassifyConfig;

pub(crate) trait RawEmbedder: Send + Sync {
    fn embed(&self, text: &str) -> io::Result<Vec<f32>>;
}

pub(crate) struct LoadedState {
    pub(crate) raw: Box<dyn RawEmbedder>,
    pub(crate) prototypes: Vec<(String, Vec<f32>)>,
}

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

#[path = "tract_embed_algo.rs"]
mod algo;

#[cfg(test)]
pub(crate) use self::algo::best_match;
pub(crate) use self::algo::normalize;
pub(crate) use self::algo::{SEQ, pad_i64};

#[expect(clippy::missing_errors_doc, reason = "配置错误经统一 io::Result 传播")]
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
#[path = "tract_embed_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "tract_embed_tests.rs"]
mod tests;
