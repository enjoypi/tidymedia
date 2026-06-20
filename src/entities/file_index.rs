use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::FixedOffset;
use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};
use tracing::warn;

use super::backend::{Backend, EntryKind};
use super::common;
use super::exif;
use super::file_info::Info;
use super::media_time;
use super::threadpool::install_io;
use super::uri::Location;
// 测试 helper `Index::visit_dir` 需要构造 LocalBackend instance。仅 #[cfg(test)]
// 下引用 adapters，生产代码 visit_location 走 backend trait 注入（CA 规则）。
#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;

const FEATURE_INDEX: &str = "index";

/// P3 sidecar 等外部时间候选的发现函数（依赖倒置：协议解析在 adapters 层，
/// entities 只接收转换好的 [`media_time::Candidate`]）。
/// 普通 fn 指针即可——provider 无状态、`Send + Sync`、可直接进 rayon 并行。
pub type CandidateProvider = fn(&Location, &Arc<dyn Backend>) -> Vec<media_time::Candidate>;

/// 一组重复文件：相同 size + 相同 content hash。size 仅 metadata，组身份由 paths 决定。
/// 避免旧 `BTreeMap<u64, Vec<Utf8PathBuf>>` 用 size 作唯一键导致同 size 不同内容互相覆盖。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size: u64,
    pub paths: Vec<Utf8PathBuf>,
}

/// 扫描目录时累计的非致命跳过/错误计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VisitStats {
    /// 0 `字节文件（Info::from` 拒收）
    pub skipped_empty: u64,
    /// 走到了文件但 `Info::from` 失败（权限/IO/symlink target 失效等）
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

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Index {
    /// 零依赖构造：Index 不再绑定单一 Backend；每条 [`Info`] 自带其 backend 句柄，
    /// 跨 scheme 索引由调用方按需 `visit_location(loc, backend)` 多次注入。
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
        for info in self.files.values() {
            bytes_read += info.bytes_read();
        }

        bytes_read
    }

    /// 判等流程：先用 `fast_hash` 找 bucket，size 必须相同，再按 `secure` 选择 hash：
    /// `true` → SHA-512（用于 copy/move 这种涉及物理修改的判等，杜绝碰撞）
    /// `false` → `xxh3_64（用于` find 默认快速模式）
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
        self.similar_files
            .par_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .map(|(_, paths)| {
                let mut same = HashMap::new();
                for path in paths {
                    let info = self
                        .files
                        .get(path)
                        .expect("internal: similar_files entries must point to a known file");
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

    pub fn search_same(&self) -> Vec<DuplicateGroup> {
        let results: Vec<_> = self.calc_same(super::file_info::Info::secure_hash);
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> Vec<DuplicateGroup> {
        let results: Vec<_> = self.calc_same(super::file_info::Info::calc_full_hash);
        Self::filter_and_sort(&results)
    }

    // 返回 Vec<DuplicateGroup> 而非 BTreeMap<size, …>：旧实现以 size 作 Map key，
    // 两组不同内容但相同 size 的重复集会互相覆盖（content 哈希一致才是同组的判据，
    // 见 calc_same 的 (size, hash) 复合 key）。Vec 形式保留每组独立性，size 仅作 metadata。
    // 排序：size 降序（render_script 沿用 iter().rev()-style 大文件先报）；size 相同时
    // 按组内首路径字典序，保证输出稳定。
    fn filter_and_sort<T>(map: &[HashMap<(u64, T), HashSet<Utf8PathBuf>>]) -> Vec<DuplicateGroup> {
        let mut groups: Vec<DuplicateGroup> = Vec::new();
        for same in map {
            for ((size, _), paths) in same {
                if paths.len() > 1 {
                    let mut v: Vec<_> = paths.iter().cloned().collect();
                    v.sort();
                    groups.push(DuplicateGroup {
                        size: *size,
                        paths: v,
                    });
                }
            }
        }
        groups.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.paths.cmp(&b.paths)));
        groups
    }

    /// 移除 prefix 目录下（含恰等）的全部条目，返回移除数。
    /// 用于 copy/move 的重叠保护：output 位于 source 子树内时，把已归档文件从
    /// source 索引剔除，避免被再次复制或在 move 模式下被当作重复副本删除。
    /// `similar_files` 反向同步清理——残留 bucket 指针会让 [`Self::exists`] panic。
    pub fn remove_under_prefix(&mut self, prefix: &str) -> usize {
        let to_remove: Vec<Utf8PathBuf> = self
            .files
            .keys()
            .filter(|p| common::under_prefix(p.as_str(), prefix))
            .cloned()
            .collect();
        // to_remove 刚取自 files.keys() → remove 必 Some，filter_map 折叠该不变式；
        // bucket 清理的防御分支收敛进 detach_from_bucket 以便直测（不可达侧无法
        // 经本入口触发，见该 fn 注释）。
        let removed: Vec<Info> = to_remove
            .iter()
            .filter_map(|p| self.files.remove(p))
            .collect();
        for info in &removed {
            Self::detach_from_bucket(&mut self.similar_files, info.fast_hash, &info.full_path);
        }
        removed.len()
    }

    /// 把 path 从 `fast_hash` bucket 摘除，空 bucket 整体移除。bucket 缺失时静默
    /// 容忍——调用方不变式（[`Self::add`] 同步建 bucket）下不可达，独立成 fn 供
    /// 测试直接喂「bucket 缺失」输入覆盖防御分支。
    fn detach_from_bucket(
        similar: &mut HashMap<u64, HashSet<Utf8PathBuf>>,
        hash: u64,
        path: &Utf8PathBuf,
    ) {
        if let Some(bucket) = similar.get_mut(&hash) {
            bucket.remove(path);
            if bucket.is_empty() {
                similar.remove(&hash);
            }
        }
    }

    pub fn add(&mut self, info: Info) -> &Info {
        use std::collections::hash_map::Entry;
        match self.files.entry(info.full_path.clone()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(slot) => {
                self.similar_files
                    .entry(info.fast_hash)
                    .or_default()
                    .insert(info.full_path.clone());
                slot.insert(info)
            }
        }
    }

    #[cfg(test)]
    pub fn insert(&mut self, path: &str) -> std::io::Result<&Info> {
        let info = Info::from(path)?;
        Ok(self.add(info))
    }

    /// 旧入口：本地路径字符串。`visit_location` 的 Local shim，让测试
    /// 不必感知 [`Location`] 类型。生产路径直接走 [`Self::visit_location`]。
    /// 相对路径先 canonicalize 成绝对路径，让 backend.walk 输出的 entry 与 Info 内
    /// `full_path` 字段保持"全路径"语义（旧 `Info::from` 的不变量）。
    #[cfg(test)]
    pub fn visit_dir(&mut self, path: &str) {
        // canonicalize 失败（路径不存在）回退到原字符串，让 walker 自身报 walker_error
        let root = match super::file_info::full_path(path) {
            Ok(p) => p,
            Err(_) => Utf8PathBuf::from(path),
        };
        let backend = LocalBackend::arc();
        self.visit_location(&Location::Local(root), &backend);
    }

    /// Backend Gateway 入口：扫描 `root` 下所有文件并入索引。`backend` 显式入参，
    /// 让单 [`Index`] 实例可承载多 scheme（先 `visit_location(smb_root, smb_be)`
    /// 再 `visit_location(local_root, local_be)`），每条 [`Info`] 的 `Info.backend`
    /// 沿用调用时传入的 backend。
    ///
    /// 错误处理与原 `visit_dir` 等价：
    /// - walker 自身 Err（缺路径、非 UTF-8、权限）→ `walker_errors += 1`
    /// - 0 字节文件 → `skipped_empty += 1`
    /// - `Info::open` 失败（chmod 000 / 中途删除等）→ `skipped_unreadable += 1`
    ///
    pub fn visit_location(&mut self, root: &Location, backend: &Arc<dyn Backend>) {
        let mut locs: Vec<Location> = Vec::new();
        for entry_res in backend.walk(root) {
            let entry = match entry_res {
                Ok(e) => e,
                Err(e) => {
                    self.stats.walker_errors += 1;
                    let root_str = root.display();
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "walker_error",
                        root = %root_str,
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

        // 跑在 I/O 专用线程池（CPU × 4，clamp [8, 64]）：远端 backend 的
        // Info::open → metadata + open_read + fast_hash_stream 是同步阻塞 IO，
        // 走全局 rayon 池会让远端 RTT 占满 CPU 核数线程让后续 CPU-bound 阶段
        // 饿死。本地 backend 也受益（更高并发隐藏 stat 抖动）。
        let infos: Vec<_> = install_io(|| {
            locs.par_iter()
                .map(|loc| Info::open(loc, Arc::clone(backend)))
                .collect()
        });
        for (loc, result) in locs.iter().zip(infos) {
            match result {
                Ok(info) => _ = self.add(info),
                Err(e) => {
                    self.stats.skipped_unreadable += 1;
                    let loc_str = loc.display();
                    warn!(
                        feature = FEATURE_INDEX,
                        operation = "walk",
                        result = "skipped_unreadable",
                        location = %loc_str,
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
        // 同 visit_location：Exif::open 内调 backend.open_read（远端是整文件
        // 同步下载）是 I/O-bound，包 I/O 池避免阻塞 CPU 池线程。
        install_io(|| {
            self.files.par_iter_mut().for_each(|(_, info)| {
                if let Ok(e) = exif::Exif::open(info.location(), &info.backend(), local_offset) {
                    info.set_exif(e);
                }
            });
        });
    }

    /// 并行对每个 indexed 文件调用 provider 注入额外时间候选（P3 sidecar 等），
    /// 与 `parse_exif` 同为"尽力而为"富集步骤：无 sidecar 时 provider 返空即可。
    pub fn enrich_candidates(&mut self, provider: CandidateProvider) {
        // provider 通常调 backend.read_to_string 读 sidecar（远端 stat + read），
        // 同 visit_location 是 I/O-bound，包 I/O 池。
        install_io(|| {
            self.files.par_iter_mut().for_each(|(_, info)| {
                let candidates = provider(info.location(), &info.backend());
                if !candidates.is_empty() {
                    info.add_candidates(candidates);
                }
            });
        });
    }
}

#[cfg(test)]
#[path = "file_index_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "file_index_advanced_tests.rs"]
mod advanced_tests;
