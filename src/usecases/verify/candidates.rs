use std::collections::HashMap;

use camino::Utf8PathBuf;

use super::content_diff::{basename, split_stem_ext};
#[cfg(test)]
use crate::entities::file_index::Index;

#[derive(Default)]
pub(crate) struct CandidateIndex {
    exact: HashMap<String, Vec<Utf8PathBuf>>,
    variant: HashMap<String, Vec<Utf8PathBuf>>,
}

impl CandidateIndex {
    #[cfg(test)]
    pub(crate) fn build(out: &Index) -> Self {
        let mut idx = Self::default();
        for entry in out.iter() {
            idx.insert(entry.key());
        }
        idx
    }

    pub(crate) fn build_from_paths<'a>(paths: impl IntoIterator<Item = &'a Utf8PathBuf>) -> Self {
        let mut idx = Self::default();
        for path in paths {
            idx.insert(path);
        }
        idx
    }

    fn insert(&mut self, path: &Utf8PathBuf) {
        let name = basename(path.as_str()).to_ascii_lowercase();
        if let Some(key) = variant_key(&name) {
            self.variant.entry(key).or_default().push(path.clone());
        }
        self.exact.entry(name).or_default().push(path.clone());
    }

    pub(crate) fn find(&self, base: &str) -> Vec<Utf8PathBuf> {
        let base_lower = base.to_ascii_lowercase();
        let mut found = self.exact.get(&base_lower).cloned().unwrap_or_default();
        if let Some(v) = self.variant.get(&base_lower) {
            found.extend(v.iter().cloned());
        }
        found
    }
}

fn variant_key(name_lower: &str) -> Option<String> {
    let (stem, ext) = split_stem_ext(name_lower);
    let (head, digits) = stem.rsplit_once('_')?;
    if digits.is_empty() {
        return None;
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{head}{ext}"))
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod tests;
