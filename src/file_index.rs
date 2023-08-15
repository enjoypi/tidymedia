use crate::file_checksum::FileChecksum;
use std::collections::{HashMap, HashSet};

pub struct FileIndex {
    // fast checksum -> file path, maybe same fast checksum
    fast_checksums: HashMap<u64, HashSet<String>>,
    files: HashMap<String, FileChecksum>, // file path -> file checksum
}

impl FileIndex {
    pub fn new() -> FileIndex {
        FileIndex {
            files: HashMap::new(),
            fast_checksums: HashMap::new(),
        }
    }

    pub fn get(&self, path: &str) -> Option<&FileChecksum> {
        self.files.get(path)
    }

    pub fn insert(&mut self, path: &str) -> std::io::Result<&FileChecksum> {
        let checksum = FileChecksum::new(path)?;

        let file_existed = self.files.get(checksum.path.as_str()).is_some();

        if file_existed {
            Ok(&self.files[&checksum.path])
        } else {
            self.fast_checksums
                .entry(checksum.fast)
                .or_insert(HashSet::new())
                .insert(checksum.path.clone());

            Ok(self.files.entry(checksum.path.clone()).or_insert(checksum))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::file_index::FileIndex;

    #[test]
    fn new() {
        let mut index = FileIndex::new();
        let checksum = index.insert("README.md").unwrap();
        assert_eq!(checksum.path, "/Users/user/prj/tidy/tidymedia/README.md");
        const FAST: u64 = 14067286713656012073;
        assert_eq!(checksum.fast, FAST);
    }
}
