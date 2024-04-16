use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::io;

use rayon::prelude::*;
use tracing::error;

use super::file_checksum::FileChecksum;

pub struct FileIndex {
    // fast checksum -> file path, maybe same fast checksum
    pub fast_checksums: HashMap<u64, HashSet<String>>,
    pub files: HashMap<String, FileChecksum>, // file path -> file checksum
}

impl FileIndex {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            fast_checksums: HashMap::new(),
        }
    }

    pub fn get(&self, checksum: u64) -> Option<&HashSet<String>> {
        self.fast_checksums.get(&checksum)
    }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for (_, checksum) in self.files.iter() {
            bytes_read += checksum.bytes_read;
        }

        bytes_read
    }

    pub fn calc_same<F, T>(&self, calc: F) -> Vec<HashMap<(u64, T), HashSet<String>>>
    where
        F: Fn(&mut FileChecksum) -> io::Result<T> + Send + Sync,
        T: Eq + Hash + Send,
    {
        let multiple: HashMap<_, _> = self
            .fast_checksums
            .iter()
            .filter(|(_, paths)| paths.len() > 1)
            .collect();

        multiple
            .par_iter()
            .map(|(_, paths)| {
                let mut same = HashMap::new();
                for path in paths.iter() {
                    let mut checksum = self.files.get(path).unwrap().clone();
                    if let Ok(key) = calc(&mut checksum) {
                        same.entry((checksum.size, key))
                            .or_insert_with(HashSet::new)
                            .insert(path.clone());
                    }
                }
                same
            })
            .collect::<Vec<_>>()
    }

    pub fn search_same(&mut self) -> BTreeMap<u64, Vec<String>> {
        let results: Vec<_> = self.calc_same(|checksum| checksum.calc_secure());
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> BTreeMap<u64, Vec<String>> {
        let results: Vec<_> = self.calc_same(|checksum| checksum.calc_full());
        Self::filter_and_sort(&results)
    }

    fn filter_and_sort<T>(
        map: &[HashMap<(u64, T), HashSet<String>>],
    ) -> BTreeMap<u64, Vec<String>> {
        let mut result = BTreeMap::new();

        for same in map.iter() {
            for ((key, _), paths) in same {
                if paths.len() > 1 {
                    let mut v: Vec<_> = paths.clone().into_iter().collect();
                    v.sort();
                    result.insert(*key, v);
                }
            }
        }

        result
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

    #[cfg(test)]
    pub fn insert(&mut self, path: &str) -> std::io::Result<&FileChecksum> {
        let checksum = FileChecksum::new(path)?;
        self.add(checksum)
    }

    pub fn visit_dir(&mut self, path: &str) {
        use ignore::Walk;

        let paths: Vec<_> = Walk::new(path)
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path().to_owned())
            .collect();

        let checksums = paths
            .par_iter()
            .map(|path| FileChecksum::new_path(path))
            .collect::<Vec<_>>();

        for result in checksums {
            match result {
                Ok(checksum) => _ = self.add(checksum),
                Err(ref e) if e.kind() == std::io::ErrorKind::Other => continue,
                Err(e) => {
                    error!("{}", e)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::super::test_common as common;
    use super::FileIndex;

    #[test]
    fn insert() -> common::Result {
        let mut index = FileIndex::new();
        let checksum = index.insert(common::DATA_SMALL)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(common::DATA_SMALL)
                .unwrap()
                .to_str()
                .unwrap() // .strip_prefix("\\\\?\\")
                          // .unwrap()
        );
        assert_eq!(checksum.short, common::DATA_SMALL_WYHASH);
        assert_eq!(checksum.full, common::DATA_SMALL_XXHASH);

        let mut new_checksum = checksum.clone();
        assert_eq!(new_checksum.calc_secure()?, common::data_small_sha512());

        Ok(())
    }

    #[test]
    fn search_same() -> common::Result {
        let mut index = FileIndex::new();
        index.visit_dir(common::DATA_DIR);

        let same: BTreeMap<u64, _> = index.search_same();
        assert_eq!(same.len(), 2);
        assert_eq!(same[&common::DATA_LARGE_LEN].len(), 2);

        assert_eq!(
            same[&common::DATA_LARGE_LEN][0],
            fs::canonicalize(common::DATA_LARGE)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                    // .unwrap()
        );
        assert_eq!(
            same[&common::DATA_LARGE_LEN][1],
            fs::canonicalize(common::DATA_LARGE_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                         // .unwrap()
        );
        assert_eq!(
            same[&common::DATA_SMALL_LEN][0],
            fs::canonicalize(common::DATA_SMALL)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                    // .unwrap()
        );
        assert_eq!(
            same[&common::DATA_SMALL_LEN][1],
            fs::canonicalize(common::DATA_SMALL_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                         // .unwrap()
        );

        Ok(())
    }
}
