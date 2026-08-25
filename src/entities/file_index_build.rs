use std::collections::HashSet;
use std::hash::Hash;
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use parking_lot::Mutex;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::warn;

use super::{DuplicateGroup, FEATURE_INDEX, Index};
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::common;
use crate::entities::file_info::Info;
use crate::entities::threadpool::install_io;
use crate::entities::uri::Location;
// 测试 helper `Index::visit_dir` 需要构造 LocalBackend instance。仅 #[cfg(test)]
// 下引用 adapters，生产代码 visit_location 走 backend trait 注入（CA 规则）。
#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;

impl Index {
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
        let results = self.calc_same(crate::entities::file_info::Info::secure_hash);
        Self::filter_and_sort(&results)
    }

    pub fn fast_search_same(&self) -> Vec<DuplicateGroup> {
        let results = self.calc_same(crate::entities::file_info::Info::calc_full_hash);
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
    pub(super) fn detach_from_bucket(
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
        let root = match crate::entities::file_info::full_path(path) {
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
}
