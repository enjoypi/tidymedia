use std::collections::HashSet;
use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::mapref::one::Ref;
use dashmap::{DashMap, DashSet};
use parking_lot::Mutex;

use super::file_info::Info;
use super::uri::Location;

const FEATURE_INDEX: &str = "index";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size: u64,
    pub paths: Vec<Utf8PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VisitStats {
    pub skipped_empty: u64,
    pub skipped_unreadable: u64,
    pub walker_errors: u64,
    pub skipped_by_ext: u64,
}

pub struct Index {
    similar_files: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>,
    files: DashMap<Utf8PathBuf, Info>,
    names: DashSet<String>,
    stats: VisitStats,
}

fn target_key(loc: &Location) -> Utf8PathBuf {
    match loc {
        Location::Local(p) => p.clone(),
        other @ Location::Smb { .. } => other.display().into(),
    }
}

fn name_key(key: &Utf8Path) -> String {
    key.components()
        .map(|c| c.as_str().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

impl fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:#?}", self.files)?;
        Ok(())
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Index {
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            similar_files: DashMap::new(),
            names: DashSet::new(),
            stats: VisitStats::default(),
        }
    }

    pub fn stats(&self) -> VisitStats {
        self.stats
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn get(&self, key: &Utf8Path) -> Option<Ref<'_, Utf8PathBuf, Info>> {
        self.files.get(key)
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, Utf8PathBuf, Info> {
        self.files.iter()
    }

    #[must_use]
    pub fn into_infos(self) -> Vec<Info> {
        self.files.into_iter().map(|(_, info)| info).collect()
    }

    #[must_use]
    pub fn similar_paths(&self, fast_hash: u64) -> Vec<Utf8PathBuf> {
        self.similar_files
            .get(&fast_hash)
            .map_or_default(|bucket| bucket.value().lock().iter().cloned().collect())
    }

    #[must_use]
    pub fn similar_bucket_count(&self) -> usize {
        self.similar_files.len()
    }

    pub fn try_reserve_target(&self, loc: &Location) -> bool {
        self.names.insert(name_key(&target_key(loc)))
    }

    pub(super) fn reserve_name(&self, key: &Utf8Path) {
        self.names.insert(name_key(key));
    }

    pub fn some_files(&self, n: usize) -> Vec<Utf8PathBuf> {
        let mut ret: Vec<Utf8PathBuf> = self
            .files
            .iter()
            .take(n)
            .map(|kv| kv.key().clone())
            .collect();
        ret.sort();
        ret
    }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for kv in &self.files {
            bytes_read += kv.value().bytes_read();
        }

        bytes_read
    }
}

#[path = "file_index_build.rs"]
mod build;

#[path = "file_index_enrich.rs"]
mod enrich;

pub use self::enrich::{CandidateProvider, TextClassifyProvider};

#[cfg(test)]
#[path = "file_index_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "file_index_advanced_tests.rs"]
mod advanced_tests;

#[cfg(test)]
#[path = "file_index_enrich_tests.rs"]
mod enrich_tests;

#[cfg(test)]
#[path = "file_index_tests_common.rs"]
pub(crate) mod tests_common;
