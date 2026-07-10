use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::FixedOffset;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::Ref;
use parking_lot::Mutex;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
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

/// F1 并行化：`DashMap` 让 `exists`/`add`/`remove_under_prefix` 全 `&self`，
/// `run_copy_loop` 才能在 `par_iter` 内共享 `&Index`。`similar_files` 桶用
/// `Mutex<HashSet>` 而非嵌套 `DashSet`——dashmap 同一 shard 递归拿锁会 deadlock
/// （issue #79）。
pub struct Index {
    // fast hash -> file path, maybe same fast hash
    similar_files: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>,
    // file path -> file meta
    files: DashMap<Utf8PathBuf, Info>,
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            similar_files: DashMap::new(),
            stats: VisitStats::default(),
        }
    }

    pub fn stats(&self) -> VisitStats {
        self.stats
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[cfg(test)]
    #[must_use]
    pub fn contains_key(&self, key: &Utf8Path) -> bool {
        self.files.contains_key(key)
    }

    /// 拿单条 `Info` 的 shard-lock guard；调用方持有期间禁与另一 file entry 嵌套操作
    /// （同 shard 递归 deadlock）。
    pub fn get(&self, key: &Utf8Path) -> Option<Ref<'_, Utf8PathBuf, Info>> {
        self.files.get(key)
    }

    /// 遍历所有 file 条目；元素是 shard-lock guard，短临界区内解出 `kv.value()` 后
    /// 立即消费，避免与 `add` / `remove_under_prefix` 长时并发。
    pub fn iter(&self) -> dashmap::iter::Iter<'_, Utf8PathBuf, Info> {
        self.files.iter()
    }

    /// `similar_files` 桶内所有 path 的 snapshot（clone）；调用方拿到 owned Vec 后
    /// bucket lock 立即释放，避免长时持锁与 `add` 并发冲突。
    #[must_use]
    pub fn similar_paths(&self, fast_hash: u64) -> Vec<Utf8PathBuf> {
        self.similar_files
            .get(&fast_hash)
            .map(|bucket| bucket.value().lock().iter().cloned().collect())
            .unwrap_or_default()
    }

    /// `similar_files` 桶数（不同 `fast_hash` 值的个数）；调试日志用。
    #[must_use]
    pub fn similar_bucket_count(&self) -> usize {
        self.similar_files.len()
    }

    /// 判定 target `Location` 是否已入索引：key 构造与 [`Info::cloned_at`] /
    /// [`Info::open`] 完全一致（Local = `Utf8PathBuf.clone()`；远端 = `display()`），
    /// 让 `naming::generate_unique_name` 在 dry-run 下也能识别「本次已假设写过」的
    /// 目标名，避免同 basename 多源被静默分派到同一 target 字符串致 dry-run 报表
    /// 与真跑归档路径分裂。真跑路径下同 hash src 走 `exists` 判去重，此 helper
    /// 只对「未真写但已 add `cloned_at`」的 dry-run 语义生效。
    #[must_use]
    pub fn contains_target(&self, loc: &Location) -> bool {
        let key: Utf8PathBuf = match loc {
            Location::Local(p) => p.clone(),
            other => other.display().into(),
        };
        self.files.contains_key(&key)
    }

    /// tracing debug 展示样本用；F1 后 `Info` 不再可克隆（内含 `Mutex<Lazy>`），
    /// 返 owned path 让 caller 拿到独立所有权，不与 `Index` 后续 mutation 竞态。
    pub fn some_files(&self, n: usize) -> Vec<Utf8PathBuf> {
        let mut ret: Vec<Utf8PathBuf> = self
            .files
            .iter()
            .take(n)
            .map(|kv| kv.key().clone())
            .collect();
        ret.sort();
        ret
    }

    pub fn bytes_read(&self) -> u64 {
        let mut bytes_read = 0;
        for kv in &self.files {
            bytes_read += kv.value().bytes_read();
        }

        bytes_read
    }

    /// 判等流程：先用 `fast_hash` 找 bucket，size 必须相同，再按 `secure` 选择 hash：
    /// `true` → SHA-512（用于 copy/move 这种涉及物理修改的判等，杜绝碰撞）
    /// `false` → `xxh3_64（用于` find 默认快速模式）
    ///
    /// F1 两阶段访问：先 clone bucket paths 释放 similar shard 锁 → 再挨个查 files，
    /// 严禁 similar / files shard guard 嵌套持有（同 shard 递归会 deadlock）。
    pub fn exists(&self, src_file: &Info, secure: bool) -> io::Result<Option<Utf8PathBuf>> {
        let paths = self.similar_paths(src_file.fast_hash);
        if paths.is_empty() {
            return Ok(None);
        }
        // filter_map 吸收「并发 remove 后 bucket 有 stale path」的 defensive None
        // 分支，让主循环只处理已解析的 entry；单点循环体便于覆盖率工具聚合 region。
        for entry in paths.iter().filter_map(|p| self.files.get(p)) {
            let f = entry.value();
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

    pub fn calc_same<F, T>(
        &self,
        calc: F,
    ) -> Vec<std::collections::HashMap<(u64, T), HashSet<Utf8PathBuf>>>
    where
        F: Fn(&Info) -> io::Result<T> + Send + Sync,
        T: Eq + Hash + Send,
    {
        let bucket_paths: Vec<(u64, Vec<Utf8PathBuf>)> = self
            .similar_files
            .iter()
            .filter_map(|kv| {
                let paths: Vec<Utf8PathBuf> = kv.value().lock().iter().cloned().collect();
                if paths.len() > 1 {
                    Some((*kv.key(), paths))
                } else {
                    None
                }
            })
            .collect();
        bucket_paths
            .par_iter()
            .map(|(_, paths)| {
                let mut same: std::collections::HashMap<(u64, T), HashSet<Utf8PathBuf>> =
                    std::collections::HashMap::new();
                // filter_map 吸收「并发 remove 后 bucket 内 path 已 stale」的
                // defensive None 分支；同 exists 套路让覆盖率不需 subprocess 时序精确
                // 触发 None arm。
                for (path, entry) in paths
                    .iter()
                    .filter_map(|p| self.files.get(p).map(|e| (p, e)))
                {
                    let info = entry.value();
                    if let Ok(key) = calc(info) {
                        same.entry((info.size, key))
                            .or_default()
                            .insert(path.clone());
                    }
                }
                same
            })
            .collect::<Vec<_>>()
    }

    pub fn search_same(&self) -> Vec<DuplicateGroup> {
        let results = self.calc_same(super::file_info::Info::secure_hash);
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> Vec<DuplicateGroup> {
        let results = self.calc_same(super::file_info::Info::calc_full_hash);
        Self::filter_and_sort(&results)
    }

    // 返回 Vec<DuplicateGroup> 而非 BTreeMap<size, …>：旧实现以 size 作 Map key，
    // 两组不同内容但相同 size 的重复集会互相覆盖（content 哈希一致才是同组的判据，
    // 见 calc_same 的 (size, hash) 复合 key）。Vec 形式保留每组独立性，size 仅作 metadata。
    // 排序：size 降序（render_script 沿用 iter().rev()-style 大文件先报）；size 相同时
    // 按组内首路径字典序，保证输出稳定。
    fn filter_and_sort<T>(
        map: &[std::collections::HashMap<(u64, T), HashSet<Utf8PathBuf>>],
    ) -> Vec<DuplicateGroup> {
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
    ///
    /// F1 两阶段：先 collect keys 释放 files 读锁 → 再逐个 remove 拿写锁；
    /// dashmap `iter()` 持读锁与 `remove()` 写锁若嵌套同 shard 会 deadlock。
    pub fn remove_under_prefix(&self, prefix: &str) -> usize {
        let to_remove: Vec<Utf8PathBuf> = self
            .files
            .iter()
            .filter(|kv| common::under_prefix(kv.key().as_str(), prefix))
            .map(|kv| kv.key().clone())
            .collect();
        // filter_map 吸收「并发另一 remove 已抢先删除」的 defensive None arm；
        // 计数用 inspect + count 让主循环体单一路径便于覆盖率聚合。
        to_remove
            .iter()
            .filter_map(|path| self.files.remove(path))
            .inspect(|(_, info)| {
                Self::detach_from_bucket(&self.similar_files, info.fast_hash, &info.full_path);
            })
            .count()
    }

    /// 把 path 从 `fast_hash` bucket 摘除，空 bucket 整体移除。bucket 缺失时静默
    /// 容忍——调用方不变式（[`Self::add`] 同步建 bucket）下不可达，独立成 fn 供
    /// 测试直接喂「bucket 缺失」输入覆盖防御分支。
    fn detach_from_bucket(
        similar: &DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>,
        hash: u64,
        path: &Utf8PathBuf,
    ) {
        let mut empty = false;
        if let Some(bucket) = similar.get(&hash) {
            let mut set = bucket.value().lock();
            set.remove(path);
            empty = set.is_empty();
        }
        if empty {
            similar.remove(&hash);
        }
    }

    /// F1：`&self` + `()` 返值——所有 caller 均 `_ = ...add(...)` 忽略旧返 `&Info`。
    /// 关键顺序：`files.entry` 的 slot.insert 完成后 guard 在 statement 边界 drop，
    /// 再操作 `similar_files`——避免持 files shard guard 时去拿 similar shard guard
    /// 产生同 shard 递归 deadlock 窗口。
    pub fn add(&self, info: Info) {
        let hash = info.fast_hash;
        // key 在 Vacant 分支内才 clone：Occupied 早退路径免一次白 clone（add 是
        // 索引装配热路径，每文件一次）。
        let key = match self.files.entry(info.full_path.clone()) {
            Entry::Occupied(_) => return,
            Entry::Vacant(slot) => {
                let key = slot.key().clone();
                slot.insert(info);
                key
            }
        };
        // files entry guard 已在上一 statement 边界 drop；此处独立拿 similar shard 锁。
        self.similar_files
            .entry(hash)
            .or_insert_with(|| Mutex::new(HashSet::new()))
            .value()
            .lock()
            .insert(key);
    }

    #[cfg(test)]
    pub fn insert(&self, path: &str) -> std::io::Result<()> {
        let info = Info::from(path)?;
        self.add(info);
        Ok(())
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
    /// F1：入口保持 `&mut self` 语义（顶层唯一入口，`stats` 单线程更新）；内部
    /// `add` 是 `&self`，仍可并发调用但本 fn 主循环串行走 for-loop。
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
                Ok(info) => self.add(info),
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
    /// `parse_non_media=false` 时非媒体 MIME 在 sniff 后短路为 mime-only `Exif`
    /// （跳过 office/zip 整文件解析）；调用方在过滤非媒体文件时传 false 省 IO。
    ///
    /// F1：`&mut self` 保留供顶层调用感知阶段边界；内部走 `DashMap` 的 `iter_mut`
    /// 拿各 shard 写锁（rayon `par_bridge` 上转并行 iter，跨 shard 天然无竞争）。
    pub fn parse_exif(&mut self, local_offset: FixedOffset, parse_non_media: bool) {
        // 同 visit_location：Exif::open_filtered 内调 backend.open_read（远端是
        // 整文件同步下载）是 I/O-bound，包 I/O 池避免阻塞 CPU 池线程。
        install_io(|| {
            self.files.iter_mut().par_bridge().for_each(|mut kv| {
                let info = kv.value_mut();
                if let Ok(e) = exif::Exif::open_filtered(
                    info.location(),
                    &info.backend(),
                    local_offset,
                    parse_non_media,
                ) {
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
            self.files.iter_mut().par_bridge().for_each(|mut kv| {
                let info = kv.value_mut();
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
