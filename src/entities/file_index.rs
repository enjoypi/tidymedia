use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

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

impl VisitStats {
    #[must_use]
    pub fn skipped_total(&self) -> u64 {
        self.skipped_empty + self.skipped_unreadable + self.walker_errors + self.skipped_by_ext
    }
}

pub struct Index {
    similar_files: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>,
    files: DashMap<Utf8PathBuf, Info>,
    names: Arc<DashSet<u64>>,
    stats: VisitStats,
    admission: Option<Arc<dyn Admission>>,
    overflowed: bool,
}

pub(crate) fn target_key(loc: &Location) -> Utf8PathBuf {
    match loc {
        Location::Local(p) => p.clone(),
        other @ Location::Smb { .. } => other.display().into(),
    }
}

fn name_key(key: &Utf8Path) -> u64 {
    let joined = key
        .components()
        .map(|c| c.as_str().to_lowercase())
        .collect::<Vec<_>>()
        .join("/");
    xxhash_rust::xxh3::xxh3_64(joined.as_bytes())
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
            names: Arc::new(DashSet::new()),
            stats: VisitStats::default(),
            admission: None,
            overflowed: false,
        }
    }

    #[must_use]
    pub fn with_admission(mut self, admission: Arc<dyn Admission>) -> Self {
        self.admission = Some(admission);
        self
    }

    #[must_use]
    pub fn sharing_names(mut self, names: Arc<DashSet<u64>>) -> Self {
        self.names = names;
        self
    }

    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn stats(&self) -> VisitStats {
        self.stats
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
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

    #[must_use]
    pub fn touched_count(&self) -> usize {
        self.len() + usize::try_from(self.stats.skipped_total()).unwrap_or(usize::MAX)
    }
}

#[path = "file_index_admission.rs"]
mod admission;
#[path = "file_index_build.rs"]
mod build;

pub use self::admission::{Admission, Admit};

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
