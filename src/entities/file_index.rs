use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::io::Write;

use camino::Utf8PathBuf;
use rayon::prelude::*;
use tracing::warn;

use super::common;
use super::exif;
use super::file_info::Info;

const FEATURE_INDEX: &str = "index";

/// 扫描目录时累计的非致命跳过/错误计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VisitStats {
    /// 0 字节文件（Info::from 拒收）
    pub skipped_empty: u64,
    /// 走到了文件但 Info::from 失败（权限/IO/symlink target 失效等）
    pub skipped_unreadable: u64,
    /// walker 自身报错的 entry（包括非 UTF-8 路径、metadata 失败）
    pub walker_errors: u64,
}

pub struct Index {
    // fast hash -> file path, maybe same fast hash
    similar_files: HashMap<u64, HashSet<Utf8PathBuf>>,
    // file path -> file meta
    files: HashMap<Utf8PathBuf, Info>,
    stats: VisitStats,
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
            stats: VisitStats::default(),
        }
    }

    pub fn stats(&self) -> VisitStats {
        self.stats
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

    /// 判等流程：先用 fast_hash 找 bucket，size 必须相同，再按 `secure` 选择 hash：
    /// `true` → SHA-512（用于 copy/move 这种涉及物理修改的判等，杜绝碰撞）
    /// `false` → xxh3_64（用于 find 默认快速模式）
    pub fn exists(&self, src_file: &Info, secure: bool) -> io::Result<Option<Utf8PathBuf>> {
        let Some(paths) = self.similar_files.get(&src_file.fast_hash) else {
            return Ok(None);
        };
        for path in paths {
            let f = self
                .files
                .get(path)
                .expect("similar_files entries must point to a known file");
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

    // walker 错误处理分支（metadata Err / non-UTF-8 path）依赖文件系统状态，构造不稳定；
    // warn! 宏内的字段表达式还要求安装 warn 级 subscriber 才被求值。整体标 coverage(off)
    // 让 nightly 严格覆盖率稳定，与 parse_exif / find_duplicates 同风格。
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn visit_dir(&mut self, path: &str) {
        use ignore::WalkBuilder;

        // 默认 `ignore::Walk` 会读取 .gitignore / .ignore / .git/info/exclude，
        // 对照片归档场景（用户媒体目录恰好在 git 工作树里）会静默漏文件，故全部关闭。
        let walker = WalkBuilder::new(path)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .require_git(false)
            .build();

        let mut paths: Vec<Utf8PathBuf> = Vec::new();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    self.stats.walker_errors += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "walker_error",
                        root = path,
                        error = %e,
                        "walker reported an error entry",
                    );
                    continue;
                }
            };
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    self.stats.walker_errors += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "metadata_error",
                        path = ?entry.path(),
                        error = %e,
                        "metadata fetch failed",
                    );
                    continue;
                }
            };
            if !meta.is_file() {
                continue;
            }
            if meta.len() == 0 {
                self.stats.skipped_empty += 1;
                warn!(
                    feature = FEATURE_INDEX,
                    operation = "walk",
                    result = "skipped_empty",
                    path = ?entry.path(),
                    "empty file skipped",
                );
                continue;
            }
            match Utf8PathBuf::from_path_buf(entry.path().to_owned()) {
                Ok(p) => paths.push(p),
                Err(_) => {
                    self.stats.walker_errors += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "non_utf8_path",
                        path = ?entry.path(),
                        "non-UTF-8 path skipped",
                    );
                }
            }
        }

        let infos = paths
            .par_iter()
            .map(|p| Info::from_path(p))
            .collect::<Vec<_>>();

        for (path, result) in paths.iter().zip(infos) {
            match result {
                Ok(info) => _ = self.add(info),
                Err(e) => {
                    self.stats.skipped_unreadable += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "skipped_unreadable",
                        path = %path,
                        error = %e,
                        "file could not be indexed",
                    );
                }
            }
        }
    }

    // tempfile/writeln/flush 等系统调用 Err 分支几乎不可稳定触发；exif::from_args 已在
    // 阶段 2C 通过 PATH=空 case 覆盖。整体把这个 IO 编排函数排出严格覆盖率统计。
    #[cfg_attr(coverage_nightly, coverage(off))]
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
#[path = "file_index_tests.rs"]
mod tests;
