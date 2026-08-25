use std::collections::HashSet;
use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use parking_lot::Mutex;

use super::file_info::Info;
use super::uri::Location;

const FEATURE_INDEX: &str = "index";

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
}

#[path = "file_index_build.rs"]
mod build;

#[path = "file_index_enrich.rs"]
mod enrich;

pub use self::enrich::{CandidateProvider, TextClassifyProvider};

#[cfg(test)]
#[path = "file_index_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "file_index_advanced_tests.rs"]
mod advanced_tests;
