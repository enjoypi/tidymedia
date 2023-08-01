use crate::file_checksum::Checksum;
use crate::FileChecksum;
use std::collections::{HashMap, HashSet};

pub struct FileIndex {
    fast: HashMap<u64, HashSet<Checksum>>,
    files: HashMap<String, FileChecksum>,
}

impl FileIndex {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            fast: HashMap::new(),
        }
    }

    pub fn add(&mut self, path: &str) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::media_index::FileIndex;
    #[test]
    fn new() {
        let mut _index = FileIndex::new();
    }
}
