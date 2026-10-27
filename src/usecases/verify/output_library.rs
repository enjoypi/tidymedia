use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use camino::Utf8PathBuf;
use dashmap::DashMap;
use parking_lot::Mutex;

use crate::entities::backend::{Backend, EntryKind};
use crate::entities::file_index::{Index, target_key};
use crate::entities::file_info::Info;
use crate::usecases::copy::Source;

use super::candidates::CandidateIndex;
use super::content_features::CandidateFeatures;

struct FeatureCell {
    value: Mutex<Option<Arc<CandidateFeatures>>>,
}

impl FeatureCell {
    fn new() -> Self {
        Self {
            value: Mutex::new(None),
        }
    }
}

struct LazyBucket {
    locations: Vec<crate::entities::uri::Location>,
    index: OnceLock<Index>,
}

impl LazyBucket {
    fn materialize(&self, backend: &Arc<dyn Backend>) -> &Index {
        self.index.get_or_init(|| {
            let mut index = Index::new();
            index.open_locations_sequential(&self.locations, backend);
            index
        })
    }

    fn materialize_parallel(&self, backend: &Arc<dyn Backend>) -> &Index {
        self.index.get_or_init(|| {
            let mut index = Index::new();
            index.open_locations(&self.locations, backend);
            index
        })
    }
}

pub(crate) struct OutputLibrary {
    backend: Arc<dyn Backend>,
    buckets: HashMap<u64, LazyBucket>,
    path_sizes: HashMap<Utf8PathBuf, u64>,
    cands: CandidateIndex,
    features: DashMap<Utf8PathBuf, Arc<FeatureCell>>,
}

impl OutputLibrary {
    pub(crate) fn from_source(output: &Source) -> Self {
        let (loc, backend) = output;
        let entries = if backend
            .metadata(loc)
            .is_ok_and(|m| m.kind == EntryKind::Dir)
        {
            let mut scanner = Index::new();
            scanner.scan_entries(loc, backend, &[])
        } else {
            Vec::new()
        };
        let mut grouped: HashMap<u64, Vec<_>> = HashMap::new();
        let mut path_sizes = HashMap::new();
        for entry in entries {
            path_sizes.insert(target_key(&entry.location), entry.size);
            grouped.entry(entry.size).or_default().push(entry.location);
        }
        let buckets = grouped
            .into_iter()
            .map(|(size, locations)| {
                (
                    size,
                    LazyBucket {
                        locations,
                        index: OnceLock::new(),
                    },
                )
            })
            .collect();
        let cands = CandidateIndex::build_from_paths(path_sizes.keys());
        Self {
            backend: Arc::clone(backend),
            buckets,
            path_sizes,
            cands,
            features: DashMap::new(),
        }
    }

    pub(crate) fn prepare_exact(&self, infos: &[Info]) {
        let sizes: HashSet<u64> = infos.iter().map(|info| info.size).collect();
        for (size, bucket) in &self.buckets {
            if sizes.contains(size) {
                bucket.materialize_parallel(&self.backend);
            }
        }
    }

    pub(super) fn has_exact(&self, info: &Info) -> bool {
        self.bucket_for_size(info.size)
            .is_some_and(|index| matches!(index.exists(info, true), Ok(Some(_))))
    }

    pub(super) fn candidates(&self, base: &str) -> Vec<Utf8PathBuf> {
        self.cands
            .find(base)
            .into_iter()
            .filter(|path| {
                self.bucket_for_path(path)
                    .is_some_and(|index| index.get(path).is_some())
            })
            .collect()
    }

    pub(super) fn features(
        &self,
        path: &Utf8PathBuf,
        max_bytes: u64,
    ) -> Option<Arc<CandidateFeatures>> {
        let cell = self
            .features
            .entry(path.clone())
            .or_insert_with(|| Arc::new(FeatureCell::new()))
            .clone();
        let mut cached = cell.value.lock();
        if let Some(value) = cached.as_ref() {
            return Some(value.clone());
        }
        let features = Arc::new(self.compute_features(path, max_bytes)?);
        *cached = Some(features.clone());
        Some(features)
    }

    fn compute_features(&self, path: &Utf8PathBuf, max_bytes: u64) -> Option<CandidateFeatures> {
        let bytes = self.read_candidate(path, max_bytes)?;
        let ext = path.extension().map_or_default(str::to_ascii_lowercase);
        Some(CandidateFeatures::from_bytes(&bytes, &ext))
    }

    pub(super) fn read_candidate(&self, path: &Utf8PathBuf, max_bytes: u64) -> Option<Vec<u8>> {
        let index = self.bucket_for_path(path)?;
        read_guarded_by_path(index, path, max_bytes)
    }

    fn bucket_for_size(&self, size: u64) -> Option<&Index> {
        self.buckets
            .get(&size)
            .map(|bucket| bucket.materialize(&self.backend))
    }

    fn bucket_for_path(&self, path: &Utf8PathBuf) -> Option<&Index> {
        let size = self.path_sizes.get(path)?;
        self.bucket_for_size(*size)
    }

    #[cfg(test)]
    pub(super) fn materialized_bucket_count(&self) -> usize {
        self.buckets
            .values()
            .filter(|bucket| bucket.index.get().is_some())
            .count()
    }
}

#[cfg(test)]
pub(crate) fn build_output_index(output: &Source) -> Index {
    let mut idx = Index::new();
    let (loc, backend) = output;
    if backend
        .metadata(loc)
        .is_ok_and(|m| m.kind == EntryKind::Dir)
    {
        idx.visit_location(loc, backend);
    }
    idx
}

pub(crate) fn read_guarded_by_path(
    out: &Index,
    path: &Utf8PathBuf,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    let info = out.get(path)?;
    super::content_diff::read_guarded(&info, max_bytes)
}

#[cfg(test)]
#[path = "output_library_tests.rs"]
mod tests;
