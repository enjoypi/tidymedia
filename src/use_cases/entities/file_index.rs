use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::io::Write;

use camino::Utf8PathBuf;
use rayon::prelude::*;

use super::common;
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

    pub fn some_files(&self, n: usize) -> Vec<&Info> {
        let mut ret: Vec<_> = self.files().iter().take(n).map(|x| x.1).collect();
        ret.sort_by(|x1, x2| x1.full_path.cmp(&x2.full_path));
        ret
    }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for (_, info) in self.files.iter() {
            bytes_read += info.bytes_read();
        }

        bytes_read
    }

    pub fn exists(&self, src_file: &Info) -> io::Result<Option<Utf8PathBuf>> {
        let Some(paths) = self.similar_files.get(&src_file.fast_hash) else {
            return Ok(None);
        };
        for path in paths {
            let f = self
                .files
                .get(path)
                .expect("similar_files entries must point to a known file");
            if f.size == src_file.size && f.calc_full_hash()? == src_file.calc_full_hash()? {
                return Ok(Some(f.full_path.clone()));
            }
        }
        Ok(None)
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
            .map(|entry| {
                Utf8PathBuf::from_path_buf(entry.path().to_owned())
                    .unwrap()
                    .to_path_buf()
            })
            .collect();

        let infos = paths
            .par_iter()
            .map(|path| Info::from_path(path))
            .collect::<Vec<_>>();

        for result in infos {
            match result {
                Ok(info) => _ = self.add(info),
                Err(_) => continue,
            }
        }
    }

    pub fn parse_exif(&mut self) -> common::Result<()> {
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
    use tempfile::tempdir;

    use super::super::file_info;
    use super::super::test_common as common;
    use super::Index;
    use super::Info;

    #[test]
    fn insert() -> common::Result {
        let mut index = Index::new();
        let info = index.insert(common::DATA_SMALL)?;
        let want = file_info::full_path(common::DATA_SMALL)?;
        assert_eq!(info.full_path, want);
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
        assert_eq!(same[&common::DATA_SMALL_LEN].len(), 2);

        let large_path = file_info::full_path(common::DATA_LARGE)?;
        let large_copy = file_info::full_path(common::DATA_LARGE_COPY)?;
        let small_path = file_info::full_path(common::DATA_SMALL)?;
        let small_copy = file_info::full_path(common::DATA_SMALL_COPY)?;
        assert!(same[&common::DATA_LARGE_LEN].contains(&large_path));
        assert!(same[&common::DATA_LARGE_LEN].contains(&large_copy));
        assert!(same[&common::DATA_SMALL_LEN].contains(&small_path));
        assert!(same[&common::DATA_SMALL_LEN].contains(&small_copy));
        Ok(())
    }

    #[test]
    fn parse_exif() -> common::Result {
        let mut index = Index::new();
        index.visit_dir(common::DATA_DIR);
        index.parse_exif()?;

        let png_path = file_info::full_path(common::DATA_DNS_BENCHMARK)?;
        let info = index.files.get(png_path.as_path()).unwrap();
        let exif = info.exif().unwrap();
        assert_eq!(exif.source_file(), png_path);
        assert!(exif.is_media());
        assert!(exif.media_create_date() > 0);
        assert!(exif.file_modify_date() > 0);
        Ok(())
    }

    #[test]
    fn exists_returns_none_for_unrelated() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        let other = Info::from(common::DATA_LARGE)?;
        assert!(index.exists(&other)?.is_none());
        Ok(())
    }

    #[test]
    fn exists_returns_some_for_duplicate() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        let copy = Info::from(common::DATA_SMALL_COPY)?;
        let found = index.exists(&copy)?.expect("duplicate must be detected");
        assert_eq!(found, file_info::full_path(common::DATA_SMALL)?);
        Ok(())
    }

    #[test]
    fn exists_handles_fast_hash_collision_with_different_content() -> common::Result {
        let dir = tempdir()?;
        let prefix = vec![0u8; 4096];

        let a_path = dir.path().join("a.bin");
        let mut a = prefix.clone();
        a.push(b'A');
        fs::write(&a_path, &a)?;

        let b_path = dir.path().join("b.bin");
        let mut b = prefix.clone();
        b.push(b'B');
        fs::write(&b_path, &b)?;

        let mut index = Index::new();
        index.insert(a_path.to_str().unwrap())?;

        let info_b = Info::from(b_path.to_str().unwrap())?;
        let info_a_ref = Info::from(a_path.to_str().unwrap())?;
        assert_eq!(info_a_ref.fast_hash, info_b.fast_hash);
        assert!(index.exists(&info_b)?.is_none());
        Ok(())
    }

    #[test]
    fn visit_dir_handles_nonexistent_path() {
        let mut index = Index::new();
        index.visit_dir("/no/such/directory/xyz123");
        assert_eq!(index.files().len(), 0);
    }

    #[test]
    fn visit_dir_skips_empty_files() -> common::Result {
        let dir = tempdir()?;
        let empty_path = dir.path().join("empty.bin");
        fs::write(&empty_path, b"")?;
        let real_path = dir.path().join("real.bin");
        fs::write(&real_path, b"abcdef")?;

        let mut index = Index::new();
        index.visit_dir(dir.path().to_str().unwrap());
        assert_eq!(index.files().len(), 1);
        let only = index.files().values().next().unwrap();
        assert!(only.full_path.as_str().ends_with("real.bin"));
        Ok(())
    }

    #[test]
    fn add_idempotent_on_same_path() -> common::Result {
        let mut index = Index::new();
        let first = Info::from(common::DATA_SMALL)?;
        let key = first.full_path.clone();
        index.add(first)?;
        let again = Info::from(common::DATA_SMALL)?;
        index.add(again)?;
        assert_eq!(index.files().len(), 1);
        assert!(index.files().contains_key(&key));
        Ok(())
    }

    #[test]
    fn some_files_sorts_and_limits() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        index.insert(common::DATA_LARGE)?;
        index.insert(common::DATA_DNS_BENCHMARK)?;
        let two = index.some_files(2);
        assert_eq!(two.len(), 2);
        assert!(two[0].full_path <= two[1].full_path);
        Ok(())
    }

    #[test]
    fn bytes_read_sums_individual() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        index.insert(common::DATA_LARGE)?;
        let total: u64 = index.files().values().map(|f| f.bytes_read()).sum();
        assert_eq!(index.bytes_read(), total);
        Ok(())
    }

    #[test]
    fn parse_exif_empty_index_ok() -> common::Result {
        let mut index = Index::new();
        index.parse_exif()?;
        assert_eq!(index.files().len(), 0);
        Ok(())
    }

    #[test]
    fn fast_search_same_matches_search_same() -> common::Result {
        let mut index = Index::new();
        index.visit_dir(common::DATA_DIR);
        let secure: BTreeMap<u64, _> = index.search_same();
        let fast: BTreeMap<u64, _> = index.fast_search_same();
        assert_eq!(secure, fast);
        Ok(())
    }

    #[test]
    fn index_debug_format_renders_files() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        let dbg = format!("{:?}", index);
        assert!(dbg.contains("data_small"));
        Ok(())
    }

    #[test]
    fn similar_files_groups_collisions() -> common::Result {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL)?;
        index.insert(common::DATA_SMALL_COPY)?;
        let group = index
            .similar_files()
            .get(&common::DATA_SMALL_WYHASH)
            .expect("collision group present");
        assert_eq!(group.len(), 2);
        let small = file_info::full_path(common::DATA_SMALL)?;
        let small_copy = file_info::full_path(common::DATA_SMALL_COPY)?;
        assert!(group.contains(&small));
        assert!(group.contains(&small_copy));
        // 让 Utf8Path import 仍被使用
        let _ = Utf8Path::new(common::DATA_SMALL);
        Ok(())
    }
}
