use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::FixedOffset;
use rayon::prelude::*;
use tracing::warn;

use super::backend::local::LocalBackend;
use super::backend::{Backend, EntryKind};
use super::exif;
use super::file_info::Info;
use super::uri::Location;

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
    backend: Arc<dyn Backend>,
}

impl fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:#?}", self.files)?;
        Ok(())
    }
}

impl Index {
    pub fn new() -> Self {
        Self::with_backend(LocalBackend::arc())
    }

    /// Backend Gateway 入口：调用方注入自定义后端（fake / 远端）。`Index::new()` 是
    /// Local 默认 shim。
    pub fn with_backend(backend: Arc<dyn Backend>) -> Self {
        Self {
            files: HashMap::new(),
            similar_files: HashMap::new(),
            stats: VisitStats::default(),
            backend,
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

    /// 旧入口：本地路径字符串。`visit_location` 的 Local shim，让现有 use cases / 测试
    /// 不必感知 [`Location`] 类型。
    /// 相对路径先 canonicalize 成绝对路径，让 backend.walk 输出的 entry 与 Info 内
    /// `full_path` 字段保持"全路径"语义（旧 Info::from 的不变量）。
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn visit_dir(&mut self, path: &str) {
        // canonicalize 失败（路径不存在）回退到原字符串，让 walker 自身报 walker_error
        let root = super::file_info::full_path(path).unwrap_or_else(|_| Utf8PathBuf::from(path));
        self.visit_location(&Location::Local(root));
    }

    /// Backend Gateway 入口：扫描 `root` 下所有文件并入索引。
    /// 错误处理与原 `visit_dir` 等价：
    /// - walker 自身 Err（缺路径、非 UTF-8、权限）→ `walker_errors += 1`
    /// - 0 字节文件 → `skipped_empty += 1`
    /// - Info::open 失败（chmod 000 / 中途删除等）→ `skipped_unreadable += 1`
    ///
    /// 函数体内分支多数依赖文件系统状态构造，且 `warn!` 字段表达式要求安装
    /// subscriber 才被求值。整体标 coverage(off)，沿用旧 visit_dir 的覆盖率策略。
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn visit_location(&mut self, root: &Location) {
        let mut locs: Vec<Location> = Vec::new();
        for entry_res in self.backend.walk(root) {
            let entry = match entry_res {
                Ok(e) => e,
                Err(e) => {
                    self.stats.walker_errors += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "walker_error",
                        root = %root.display(),
                        error = %e,
                        "walker reported an error entry",
                    );
                    continue;
                }
            };
            if entry.kind != EntryKind::File {
                continue;
            }
            if entry.size == 0 {
                self.stats.skipped_empty += 1;
                warn!(
                    feature = FEATURE_INDEX,
                    operation = "walk",
                    result = "skipped_empty",
                    location = %entry.location.display(),
                    "empty file skipped",
                );
                continue;
            }
            locs.push(entry.location);
        }

        let backend = Arc::clone(&self.backend);
        let infos: Vec<_> = locs
            .par_iter()
            .map(|loc| Info::open(loc, Arc::clone(&backend)))
            .collect();
        for (loc, result) in locs.iter().zip(infos) {
            match result {
                Ok(info) => _ = self.add(info),
                Err(e) => {
                    self.stats.skipped_unreadable += 1;
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "skipped_unreadable",
                        location = %loc.display(),
                        error = %e,
                        "file could not be indexed",
                    );
                }
            }
        }
    }

    /// 并行对每个 indexed 文件用 nom-exif + infer 读取元数据；解析失败的文件被
    /// 静默跳过（"尽力而为"语义）。从不返回错误。
    /// `local_offset` 用于解释 EXIF 内无时区的 NaiveDateTime（相机本地时区）。
    pub fn parse_exif(&mut self, local_offset: FixedOffset) {
        self.files.par_iter_mut().for_each(|(path, info)| {
            if let Ok(e) = exif::Exif::from_path_with_offset(path, local_offset) {
                info.set_exif(e);
            }
        });
    }
}

#[cfg(test)]
#[path = "file_index_tests.rs"]
mod tests;
