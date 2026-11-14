use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use parking_lot::Mutex;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::{debug, warn};

use super::{Admit, DuplicateGroup, FEATURE_INDEX, Index};
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::common;
use crate::entities::file_info::Info;
use crate::entities::threadpool::install_io;
use crate::entities::uri::Location;

impl Index {
    pub fn exists(&self, src_file: &Info, secure: bool) -> io::Result<Option<Utf8PathBuf>> {
        let paths = self.similar_paths(src_file.fast_hash);
        if paths.is_empty() {
            return Ok(None);
        }
        for entry in paths.iter().filter_map(|p| self.files.get(p)) {
            let f = entry.value();
            if f.size != src_file.size {
                continue;
            }
            let equal = if secure {
                f.secure_hash()? == src_file.secure_hash()?
            } else {
                f.calc_full_hash()? == src_file.calc_full_hash()?
            };
            if equal {
                return Ok(Some(f.full_path.clone()));
            }
        }
        Ok(None)
    }

    pub fn calc_same<F, T>(&self, calc: F) -> HashMap<(u64, T), HashSet<Utf8PathBuf>>
    where
        F: Fn(&Info) -> io::Result<T> + Send + Sync,
        T: Eq + Hash + Send,
    {
        let mut by_size: HashMap<(u64, u64), Vec<Utf8PathBuf>> = HashMap::new();
        for bucket in &self.similar_files {
            let paths = bucket.value().lock();
            if paths.len() < 2 {
                continue;
            }
            for (path, size) in paths
                .iter()
                .filter_map(|p| self.files.get(p).map(|e| (p, e.value().size)))
            {
                by_size
                    .entry((*bucket.key(), size))
                    .or_default()
                    .push(path.clone());
            }
        }
        let peers: Vec<Utf8PathBuf> = by_size
            .into_values()
            .filter(|paths| paths.len() > 1)
            .flatten()
            .collect();
        let keyed: Vec<(Utf8PathBuf, u64, T)> = install_io(|| {
            peers
                .par_iter()
                .filter_map(|path| {
                    self.files.get(path).and_then(|entry| {
                        let info = entry.value();
                        calc(info).ok().map(|key| (path.clone(), info.size, key))
                    })
                })
                .collect()
        });
        let mut same: HashMap<(u64, T), HashSet<Utf8PathBuf>> = HashMap::new();
        for (path, size, key) in keyed {
            same.entry((size, key)).or_default().insert(path);
        }
        same
    }

    pub fn search_same(&self) -> Vec<DuplicateGroup> {
        let results = self.calc_same(crate::entities::file_info::Info::secure_hash);
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> Vec<DuplicateGroup> {
        let results = self.calc_same(crate::entities::file_info::Info::calc_full_hash);
        Self::filter_and_sort(&results)
    }

    fn filter_and_sort<T>(same: &HashMap<(u64, T), HashSet<Utf8PathBuf>>) -> Vec<DuplicateGroup> {
        let mut groups: Vec<DuplicateGroup> = Vec::new();
        for ((size, _), paths) in same {
            if paths.len() > 1 {
                let mut v: Vec<_> = paths.iter().cloned().collect();
                v.sort();
                groups.push(DuplicateGroup {
                    size: *size,
                    paths: v,
                });
            }
        }
        sort_groups(&mut groups);
        groups
    }

    pub fn remove_under_prefix(&self, prefix: &str) -> usize {
        let prefix = common::key_from_display(prefix);
        let to_remove: Vec<Utf8PathBuf> = self
            .files
            .iter()
            .filter(|kv| {
                common::under_prefix(&common::key_from_display(kv.key().as_str()), &prefix)
            })
            .map(|kv| kv.key().clone())
            .collect();
        to_remove
            .iter()
            .filter_map(|path| self.files.remove(path))
            .inspect(|(_, info)| {
                Self::detach_from_bucket(&self.similar_files, info.fast_hash, &info.full_path);
            })
            .count()
    }

    pub(super) fn detach_from_bucket(
        similar: &DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>,
        hash: u64,
        path: &Utf8PathBuf,
    ) {
        let mut empty = false;
        if let Some(bucket) = similar.get(&hash) {
            let mut set = bucket.value().lock();
            set.remove(path);
            empty = set.is_empty();
        }
        if empty {
            similar.remove(&hash);
        }
    }

    pub fn add(&self, info: Info) {
        let hash = info.fast_hash;
        let key = match self.files.entry(info.full_path.clone()) {
            Entry::Occupied(_) => return,
            Entry::Vacant(slot) => {
                let key = slot.key().clone();
                slot.insert(info);
                key
            }
        };
        self.reserve_name(&key);
        self.similar_files
            .entry(hash)
            .or_insert_with(|| Mutex::new(HashSet::new()))
            .value()
            .lock()
            .insert(key);
    }

    pub fn visit_location(&mut self, root: &Location, backend: &Arc<dyn Backend>) {
        self.visit_location_skipping(root, backend, &[]);
    }

    pub fn visit_location_skipping(
        &mut self,
        root: &Location,
        backend: &Arc<dyn Backend>,
        skip_ext: &[String],
    ) {
        let entries = self.scan_entries(root, backend, skip_ext);
        let locs: Vec<Location> = entries.into_iter().map(|entry| entry.location).collect();
        self.open_locations(&locs, backend);
    }

    pub fn scan_entries(
        &mut self,
        root: &Location,
        backend: &Arc<dyn Backend>,
        skip_ext: &[String],
    ) -> Vec<crate::entities::backend::Entry> {
        let mut entries = Vec::new();
        for entry_res in backend.walk(root) {
            let shared = self
                .admission
                .as_ref()
                .is_none_or(|a| a.counts_shared_stats());
            let entry = match entry_res {
                Ok(e) => e,
                Err(e) => {
                    self.stats.walker_errors += u64::from(shared);
                    let root_str = root.display();
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "walker_error",
                        root = %root_str,
                        error = %e,
                        "walker reported an error entry",
                    );
                    continue;
                }
            };
            if entry.kind != EntryKind::File {
                continue;
            }
            if has_extension_in(&entry.location, skip_ext) {
                self.stats.skipped_by_ext += u64::from(shared);
                continue;
            }
            if entry.size == 0 {
                self.stats.skipped_empty += u64::from(shared);
                debug!(
                    feature = FEATURE_INDEX,
                    operation = "walk",
                    result = "skipped_empty",
                    location = %entry.location.display(),
                    "empty file skipped",
                );
                continue;
            }
            match self
                .admission
                .as_ref()
                .map_or(Admit::Take, |a| a.admit(&entry))
            {
                Admit::Take => entries.push(entry),
                Admit::Skip => {}
                Admit::Overflow => {
                    self.overflowed = true;
                    break;
                }
            }
        }
        entries
    }

    pub fn open_locations(&mut self, locs: &[Location], backend: &Arc<dyn Backend>) {
        let infos: Vec<_> = install_io(|| {
            locs.par_iter()
                .map(|loc| self.open_kept(loc, backend))
                .collect()
        });
        self.add_open_results(locs, infos);
    }

    pub fn open_locations_sequential(&mut self, locs: &[Location], backend: &Arc<dyn Backend>) {
        let infos: Vec<_> = locs
            .iter()
            .map(|loc| self.open_kept(loc, backend))
            .collect();
        self.add_open_results(locs, infos);
    }

    fn open_kept(&self, loc: &Location, backend: &Arc<dyn Backend>) -> io::Result<Option<Info>> {
        let info = Info::open(loc, Arc::clone(backend))?;
        let keep = self.admission.as_ref().is_none_or(|a| a.keep_info(&info));
        Ok(keep.then_some(info))
    }

    fn add_open_results(&mut self, locs: &[Location], infos: Vec<io::Result<Option<Info>>>) {
        for (loc, result) in locs.iter().zip(infos) {
            match result {
                Ok(Some(info)) => self.add(info),
                Ok(None) => {}
                Err(e) => {
                    self.stats.skipped_unreadable += 1;
                    let loc_str = loc.display();
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "skipped_unreadable",
                        location = %loc_str,
                        error = %e,
                        "file could not be indexed",
                    );
                }
            }
        }
    }
}

fn sort_groups(groups: &mut [DuplicateGroup]) {
    groups.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.paths.cmp(&b.paths)));
}

fn has_extension_in(loc: &Location, list: &[String]) -> bool {
    loc.path()
        .extension()
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}
