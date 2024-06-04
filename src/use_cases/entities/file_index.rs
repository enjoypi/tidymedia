use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::io::Write;

use camino::Utf8PathBuf;
use rayon::prelude::*;
use tracing::error;

use super::exif;
use super::file_info::Info;

pub struct Index {
    // fast hash -> file path, maybe same fast hash
    similar_files: HashMap<u64, HashSet<Utf8PathBuf>>,
    // file path -> file meta
    files: HashMap<Utf8PathBuf, Info>,
}

impl fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:#?}", self.files)?;
        Ok(())
    }
}

impl Index {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            similar_files: HashMap::new(),
        }
    }

    pub fn files(&self) -> &HashMap<Utf8PathBuf, Info> {
        &self.files
    }

    pub fn similar_files(&self) -> &HashMap<u64, HashSet<Utf8PathBuf>> {
        &self.similar_files
    }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for (_, info) in self.files.iter() {
            bytes_read += info.bytes_read();
        }

        bytes_read
    }

    pub fn exists(&self, src_file: &Info) -> io::Result<Option<Utf8PathBuf>> {
        match self.similar_files.get(&src_file.fast_hash) {
            Some(paths) => {
                for path in paths {
                    if let Some(f) = self.files.get(path) {
                        if f != src_file {
                            continue;
                        }

                        if f.calc_full_hash()? == src_file.calc_full_hash()? {
                            return Ok(Some(f.full_path.clone()));
                        }
                    }
                }
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn calc_same<F, T>(&self, calc: F) -> Vec<HashMap<(u64, T), HashSet<Utf8PathBuf>>>
        where
            F: Fn(&Info) -> io::Result<T> + Send + Sync,
            T: Eq + Hash + Send,
    {
        let multiple: HashMap<_, _> = self
            .similar_files
            .iter()
            .filter(|(_, paths)| paths.len() > 1)
            .collect();

        multiple
            .par_iter()
            .map(|(_, paths)| {
                let mut same = HashMap::new();
                for path in paths.iter() {
                    let info = self.files.get(path).unwrap();
                    if let Ok(key) = calc(info) {
                        same.entry((info.size, key))
                            .or_insert_with(HashSet::new)
                            .insert(path.clone());
                    }
                }
                same
            })
            .collect::<Vec<_>>()
    }

    pub fn search_same(&self) -> BTreeMap<u64, Vec<Utf8PathBuf>> {
        let results: Vec<_> = self.calc_same(|info| info.secure_hash());
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> BTreeMap<u64, Vec<Utf8PathBuf>> {
        let results: Vec<_> = self.calc_same(|info| info.calc_full_hash());
        Self::filter_and_sort(&results)
    }

    fn filter_and_sort<T>(
        map: &[HashMap<(u64, T), HashSet<Utf8PathBuf>>],
    ) -> BTreeMap<u64, Vec<Utf8PathBuf>> {
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

    pub fn add(&mut self, info: Info) -> std::io::Result<&Info> {
        let file_existed = self.files.get(&info.full_path).is_some();

        if file_existed {
            Ok(&self.files[&info.full_path])
        } else {
            self.similar_files
                .entry(info.fast_hash)
                .or_default()
                .insert(info.full_path.clone());

            Ok(self.files.entry(info.full_path.clone()).or_insert(info))
        }
    }

    #[cfg(test)]
    pub fn insert(&mut self, path: &str) -> std::io::Result<&Info> {
        let info = Info::from(path)?;
        self.add(info)
    }

    pub fn visit_dir(&mut self, path: &str) {
        use ignore::Walk;

        let paths: Vec<Utf8PathBuf> = Walk::new(path)
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.metadata().unwrap().is_file())
            .map(|entry| Utf8PathBuf::from_path_buf(entry.path().to_owned()).unwrap().to_path_buf())
            .collect();

        let infos = paths
            .par_iter()
            .map(|path| Info::from_path(path))
            .collect::<Vec<_>>();

        for result in infos {
            match result {
                Ok(info) => _ = self.add(info),
                Err(ref e) if e.kind() == std::io::ErrorKind::Other => continue,
                Err(e) => {
                    error!("{}", e)
                }
            }
        }
    }

    pub fn parse_exif(&mut self) -> Result<(), exif::ExifError> {
        // write all filenames to a file
        let mut file = tempfile::NamedTempFile::new()?;
        let mut filenames: Vec<Utf8PathBuf> = self.files.keys().cloned().collect();
        filenames.sort();

        for filename in filenames.iter() {
            // file writeln
            writeln!(&mut file, "{}", filename)?;
        }
        file.flush()?;

        let v = exif::Exif::from_args(vec!["-@", file.path().to_str().unwrap()])?;
        v.iter().for_each(|e| {
            if let Some(info) = self.files.get_mut(e.source_file()) {
                info.set_exif(e.clone());
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use camino::Utf8Path;

    use super::Index;
    use super::super::test_common as common;

    #[test]
    fn insert() -> common::Result {
        let mut index = Index::new();
        let info = index.insert(common::DATA_SMALL)?;
        assert_eq!(
            info.full_path,
            fs::canonicalize(common::DATA_SMALL)
                .unwrap()
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
                .replace('\\', "/")
        );
        assert_eq!(info.fast_hash, common::DATA_SMALL_WYHASH);

        assert_eq!(info.calc_full_hash()?, common::DATA_SMALL_XXHASH);
        assert_eq!(info.secure_hash()?, common::data_small_sha512());

        Ok(())
    }

    #[test]
    fn search_same() -> common::Result {
        let mut index = Index::new();
        index.visit_dir(common::DATA_DIR);

        let same: BTreeMap<u64, _> = index.search_same();
        assert_eq!(same.len(), 2);
        assert_eq!(same[&common::DATA_LARGE_LEN].len(), 2);

        assert_eq!(
            same[&common::DATA_LARGE_LEN][0],
            fs::canonicalize(common::DATA_LARGE)?
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
                .replace('\\', "/")
        );
        assert_eq!(
            same[&common::DATA_LARGE_LEN][1],
            fs::canonicalize(common::DATA_LARGE_COPY)?
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
                .replace('\\', "/")
        );
        assert_eq!(
            same[&common::DATA_SMALL_LEN][0],
            fs::canonicalize(common::DATA_SMALL)?
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
                .replace('\\', "/")
        );
        assert_eq!(
            same[&common::DATA_SMALL_LEN][1],
            fs::canonicalize(common::DATA_SMALL_COPY)?
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
                .replace('\\', "/")
        );

        Ok(())
    }

    #[test]
    fn parse_exif() -> common::Result {
        let mut index = Index::new();
        index.visit_dir(common::DATA_DIR);
        index.parse_exif()?;

        let full_path = Utf8Path::new(common::DATA_DNS_BENCHMARK).canonicalize_utf8()?;
        let full_path = full_path.as_str()
            .strip_prefix("\\\\?\\").unwrap()
            .replace('\\', "/");
        let info = index.files.get(Utf8Path::new(full_path.as_str())).unwrap();
        let exif = info.exif().unwrap();
        assert_eq!(exif.source_file(), full_path);
        assert_eq!(exif.file_modify_date(), 1706076164);
        assert_eq!(exif.media_create_date(), 1706076164);
        assert!(exif.is_media());

        Ok(())
    }
}
