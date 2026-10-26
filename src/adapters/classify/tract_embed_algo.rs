use std::io;

use camino::Utf8Path;

use crate::adapters::classify::tract_embed::{
    LoadedState, RawEmbedder, TractEmbedClassifier, build_prototypes,
};
use crate::adapters::classify::tract_embed_real::load_classifier;
use crate::usecases::classify::{Classification, DocumentClassifier};
use crate::usecases::config::ClassifyConfig;

pub(crate) const SEQ: usize = 256;

pub(crate) fn empty_classification() -> Classification {
    Classification {
        category: String::new(),
        score: f32::NEG_INFINITY,
    }
}

pub(crate) fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 && norm.is_finite() {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

pub(crate) fn pad_i64(v: &[u32]) -> Vec<i64> {
    let mut out: Vec<i64> = v.iter().take(SEQ).map(|&x| i64::from(x)).collect();
    out.resize(SEQ, 0);
    out
}

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

fn total_cmp_nan_as_neg_inf(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.total_cmp(&b),
    }
}

impl TractEmbedClassifier {
    pub(crate) fn ensure_loaded(&self) -> io::Result<()> {
        self.ensure_loaded_with(load_classifier)
    }

    pub(super) fn ensure_loaded_with(
        &self,
        load: fn(&ClassifyConfig) -> io::Result<Box<dyn RawEmbedder>>,
    ) -> io::Result<()> {
        let mut guard = self.state.lock();
        if guard.is_some() {
            return Ok(());
        }
        let loaded = load(&self.cfg).and_then(|raw| {
            build_prototypes(raw.as_ref(), &self.cfg)
                .map(|prototypes| LoadedState { raw, prototypes })
        })?;
        *guard = Some(loaded);
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
