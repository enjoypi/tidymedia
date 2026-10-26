use camino::{Utf8Path, Utf8PathBuf};

use super::{Index, name_key, target_key};
use crate::adapters::backend::local::LocalBackend;
use crate::entities::file_info::{Info, full_path};
use crate::entities::uri::Location;

impl Index {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[must_use]
    pub fn contains_key(&self, key: &Utf8Path) -> bool {
        self.files.contains_key(key)
    }

    #[must_use]
    pub fn contains_target(&self, loc: &Location) -> bool {
        self.names.contains(&name_key(&target_key(loc)))
    }

    pub fn insert(&self, path: &str) -> std::io::Result<()> {
        let info = Info::from(path)?;
        self.add(info);
        Ok(())
    }

    pub fn visit_dir(&mut self, path: &str) {
        let root = match full_path(path) {
            Ok(p) => p,
            Err(_) => Utf8PathBuf::from(path),
        };
        let backend = LocalBackend::arc();
        self.visit_location(&Location::Local(root), &backend);
    }
}
