use crate::file_checksum::FileChecksum;
use futures::stream::StreamExt;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::{debug, error};

const MAX_CONCURRENT_TASKS: usize = 16;

pub struct FileIndex {
    // fast checksum -> file path, maybe same fast checksum
    pub fast_checksums: HashMap<u64, HashSet<String>>,
    pub files: HashMap<String, FileChecksum>, // file path -> file checksum
}

impl FileIndex {
    pub fn new() -> FileIndex {
        FileIndex {
            files: HashMap::new(),
            fast_checksums: HashMap::new(),
        }
    }

    // pub fn get_(&self, path: &str) -> Option<&FileChecksum> {
    //     self.files.get(path)
    // }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for (_, checksum) in self.files.iter() {
            bytes_read += checksum.bytes_read;
        }

        bytes_read
    }

    pub fn search_same(&mut self) -> Vec<HashSet<String>> {
        let results: Vec<_> = self
            .fast_checksums
            .par_iter()
            .map(|(_, paths)| {
                if paths.len() <= 1 {
                    return HashMap::default();
                }

                let mut same = HashMap::new();
                for path in paths.iter() {
                    let mut checksum = self.files.get(path).unwrap().clone();
                    if let Ok(secure) = checksum.calc_secure() {
                        same.entry(secure)
                            .or_insert(HashSet::new())
                            .insert(path.clone());
                    }
                }
                same
            })
            .collect::<Vec<_>>();

        Self::filter_one(&results)
    }

    pub async fn fast_search_same(&self) -> Vec<HashSet<String>> {
        let filenames: Vec<_> =
            futures::stream::iter(self.fast_checksums.values().map(|paths| async move {
                if paths.len() <= 1 {
                    return HashMap::default();
                }
                let mut results = HashMap::new();
                for path in paths.iter() {
                    let mut checksum = self.files.get(path).unwrap().clone();
                    if let Err(e) = checksum.calc_full() {
                        debug!("{}: {}", path, e);
                        continue;
                    }
                    results
                        .entry(checksum.full)
                        .or_insert(HashSet::new())
                        .insert(path.clone());
                }
                results
            }))
            .buffer_unordered(MAX_CONCURRENT_TASKS)
            .collect::<Vec<_>>()
            .await;

        Self::filter_one(&filenames)
    }

    fn filter_one<T>(map: &[HashMap<T, HashSet<String>>]) -> Vec<HashSet<String>> {
        let mut ret = Vec::new();
        for same in map.iter() {
            for (_, paths) in same.iter() {
                if paths.len() > 1 {
                    ret.push(paths.clone());
                }
            }
        }
        ret
    }

    pub fn add(&mut self, checksum: FileChecksum) -> std::io::Result<&FileChecksum> {
        let file_existed = self.files.get(checksum.path.as_str()).is_some();

        if file_existed {
            Ok(&self.files[&checksum.path])
        } else {
            self.fast_checksums
                .entry(checksum.short)
                .or_default()
                .insert(checksum.path.clone());

            Ok(self.files.entry(checksum.path.clone()).or_insert(checksum))
        }
    }

    pub fn insert(&mut self, path: &str) -> std::io::Result<&FileChecksum> {
        let checksum = FileChecksum::new(path)?;
        self.add(checksum)
    }

    pub async fn visit_dir(&mut self, path: &Path) {
        use ignore::Walk;
        let checksums: Vec<_> = futures::stream::iter(
            Walk::new(path)
                .map(|entry| async move { FileChecksum::new_async(entry.unwrap().path()).await }),
        )
        .buffer_unordered(MAX_CONCURRENT_TASKS)
        .collect::<Vec<_>>()
        .await;

        for result in checksums {
            match result {
                Ok(checksum) => _ = self.add(checksum),
                Err(ref e) if e.kind() == std::io::ErrorKind::IsADirectory => continue,
                Err(e) => {
                    error!("{}", e)
                }
            }
        }
    }
}

impl Default for FileIndex {
    fn default() -> Self {
        FileIndex::new()
    }
}
