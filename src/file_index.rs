use crate::file_checksum::FileChecksum;
use std::collections::{HashMap, HashSet};
use std::path::Path;

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
        let mut same = HashMap::new();

        for (_, paths) in self.fast_checksums.iter() {
            if paths.len() <= 1 {
                continue;
            }

            for path in paths.iter() {
                let checksum = self.files.get_mut(path).unwrap();
                if let Ok(secure) = checksum.calc_secure() {
                    same.entry(secure)
                        .or_insert(HashSet::new())
                        .insert(path.clone());
                }
            }
        }

        let mut ret = Vec::new();
        for (_, paths) in same.iter() {
            if paths.len() > 1 {
                ret.push(paths.clone());
            }
        }

        ret
    }

    pub fn fast_search_same(&mut self) -> Vec<HashSet<String>> {
        let mut same = HashMap::new();

        for (_, paths) in self.fast_checksums.iter() {
            if paths.len() <= 1 {
                continue;
            }

            for path in paths.iter() {
                let checksum = self.files.get_mut(path).unwrap();
                if let Ok(long) = checksum.calc_full() {
                    same.entry(long)
                        .or_insert(HashSet::new())
                        .insert(path.clone());
                }
            }
        }

        let mut ret = Vec::new();
        for (_, paths) in same.iter() {
            if paths.len() > 1 {
                ret.push(paths.clone());
            }
        }

        ret
    }

    pub fn insert(&mut self, path: &str) -> std::io::Result<&FileChecksum> {
        let checksum = FileChecksum::new(path)?;

        let file_existed = self.files.get(checksum.path.as_str()).is_some();

        if file_existed {
            Ok(&self.files[&checksum.path])
        } else {
            self.fast_checksums
                .entry(checksum.short)
                .or_insert(HashSet::new())
                .insert(checksum.path.clone());

            Ok(self.files.entry(checksum.path.clone()).or_insert(checksum))
        }
    }

    pub fn visit_dir(&mut self, path: &Path) {
        use ignore::Walk;
        for result in Walk::new(path) {
            // Each item yielded by the iterator is either a directory entry or an
            // error, so either print the path or the error.
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_dir() {
                        continue;
                    }
                    match path.to_str() {
                        Some(s) => _ = self.insert(s),
                        None => continue,
                    }
                }
                Err(err) => println!("ERROR: {}", err),
            }
        }
    }
}

impl Default for FileIndex {
    fn default() -> Self {
        FileIndex::new()
    }
}
