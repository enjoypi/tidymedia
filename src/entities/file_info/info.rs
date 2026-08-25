//! `Info` 实体：按需哈希（fast/full/secure）缓存 + EXIF 持有 + 拍摄时间裁决入口。
//!
//! 时间裁决（`create_time` / `media_time_decision` / `pick_fs_fallback`）在子模块
//! `time`；本文件保留 struct 定义、访问器、哈希与克隆。
#[path = "time.rs"]
pub(super) mod time;

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::FixedOffset;
use parking_lot::Mutex;

use super::streams::{fast_hash_stream, full_hash_stream, secure_hash_stream};
use crate::entities::backend::{Backend, EntryKind, Metadata as BackendMetadata};
use crate::entities::uri::Location;
use crate::entities::{SecureHash, exif, media_time};
// 测试 helper `Info::from` 需要构造 LocalBackend instance。仅 #[cfg(test)] 下引用
// adapters，生产 `Info::open` 走 backend trait 注入（CA 规则）。
#[cfg(test)]
use super::paths::full_path;
#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lazy {
    bytes_read: u64,
    // 初始构造时 hash 字段先放 `fast_hash_stream` 的第二条 xxh3 (前 4 KiB)，
    // 直到 calc_full_hash 跑过才被替换成整文件 hash；`full` 区分这两种语义。
    full: bool,
    hash: u64,
    secure_hash: SecureHash,
}

impl Lazy {
    fn new(bytes_read: u64, hash: u64) -> Self {
        Self {
            bytes_read,
            hash,
            full: false,
            secure_hash: SecureHash::default(),
        }
    }
}

pub struct Info {
    pub fast_hash: u64,
    pub full_path: Utf8PathBuf,
    pub size: u64,

    // Backend Gateway 抽象：calc_full_hash / secure_hash 需要重新 open_read 时
    // 复用这把后端句柄；Local 下 location 与 full_path 等价，远端则以 URI 承载。
    location: Location,
    backend: Arc<dyn Backend>,

    exif: Option<exif::Exif>,
    /// P3 候选（XMP / Takeout sidecar）：协议解析在 adapters 层，经
    /// [`Self::add_candidates`] 注入；entities 只消费转换好的 [`media_time::Candidate`]。
    extra_candidates: Vec<media_time::Candidate>,
    /// 文档内容类目（copy-doc/move-doc 分类结果），经 [`Self::set_category`] 注入；
    /// None = 未分类（媒体文件 / 非 doc 命令 / 分类失败），render 时兜底 uncategorized。
    category: Option<String>,
    lazy: Mutex<Lazy>,
    meta: BackendMetadata,
}

impl std::fmt::Debug for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fast_hash: {}, size: {}\n{:#?}",
            self.fast_hash, self.size, self.exif
        )
    }
}

impl Info {
    /// 旧入口：根据本地路径字符串构造 Info。等价于以 [`LocalBackend`] 调用
    /// [`Info::open`] 的 shim；测试沿用此简短入口，生产路径走 [`Info::open`]。
    #[cfg(test)]
    pub fn from(path: &str) -> io::Result<Self> {
        let fp = full_path(path)?;
        Self::open(&Location::Local(fp), LocalBackend::arc())
    }

    /// Backend Gateway 入口：按 [`Location`] + [`Backend`] 抽象 stat 文件、读首 4 KiB
    /// 算 fast hash，并把后端句柄留住以便 [`Self::calc_full_hash`] / [`Self::secure_hash`]
    /// 复用。错误语义沿用旧 `from(&str)`：目录返回 `is a directory`、0 字节返回 `is empty`。
    pub fn open(loc: &Location, backend: Arc<dyn Backend>) -> io::Result<Self> {
        let meta = backend.metadata(loc)?;
        ensure_hashable(&meta, loc)?;
        let mut reader = backend.open_read(loc)?;
        let (bytes_read, first_hash, second_hash) = fast_hash_stream(reader.as_mut())?;
        let full_path: Utf8PathBuf = match loc {
            Location::Local(p) => p.clone(),
            other => other.display().into(),
        };
        Ok(Self {
            fast_hash: first_hash,
            full_path,
            size: meta.size,
            location: loc.clone(),
            backend,
            exif: None,
            extra_candidates: Vec::new(),
            category: None,
            lazy: Mutex::new(Lazy::new(bytes_read as u64, second_hash)),
            meta,
        })
    }

    pub fn bytes_read(&self) -> u64 {
        self.lazy.lock().bytes_read
    }

    /// 当前文件归属的 [`Location`]：Local 是绝对路径 wrap，远端是原始 URI。
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// 返回打开 Info 时使用的 backend 句柄；caller 用来对相同文件再做 IO（如 `remove_file`）。
    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::clone(&self.backend)
    }

    // cache-hit 走 `if l.full` 短路，二次调用直接复用 lazy.hash。
    // 语义由 info_open_calc_full_hash_caches_on_second_call 单元测试断言。
    pub fn calc_full_hash(&self) -> io::Result<u64> {
        let mut l = self.lazy.lock();
        if l.full {
            return Ok(l.hash);
        }
        let mut reader = self.backend.open_read(&self.location)?;
        let (bytes_read, full) = full_hash_stream(reader.as_mut())?;
        l.hash = full;
        l.bytes_read += bytes_read;
        l.full = true;
        Ok(full)
    }

    pub(super) fn full_hash(&self) -> u64 {
        self.lazy.lock().hash
    }

    // 同 calc_full_hash：cache-hit 跨 test binary 多 instance，整 fn 标 off。
    // 语义由 info_open_secure_hash_caches_on_second_call 单元测试断言。
    pub fn secure_hash(&self) -> io::Result<SecureHash> {
        let mut l = self.lazy.lock();
        if l.secure_hash != SecureHash::default() {
            return Ok(l.secure_hash);
        }
        let mut reader = self.backend.open_read(&self.location)?;
        let (bytes_read, secure) = secure_hash_stream(reader.as_mut())?;
        l.bytes_read += bytes_read;
        l.secure_hash = secure;
        Ok(secure)
    }

    #[cfg(test)]
    pub fn exif(&self) -> Option<&exif::Exif> {
        self.exif.as_ref()
    }

    /// 返回当前文件的 EXIF 数据引用（生产 + 测试均可用）。
    /// 仅在 `parse_exif` 已被调用后有值；否则为 `None`。
    pub fn exif_ref(&self) -> Option<&exif::Exif> {
        self.exif.as_ref()
    }

    pub fn set_exif(&mut self, exif: exif::Exif) {
        self.exif = Some(exif);
    }

    /// 注入外部来源（P3 sidecar 等）的时间候选；与 EXIF/文件名/mtime 候选一起
    /// 参与 [`Self::create_time`] 的 P0–P4 裁决。
    pub fn add_candidates(&mut self, candidates: Vec<media_time::Candidate>) {
        self.extra_candidates.extend(candidates);
    }

    /// 把当前 Info 的 hash / size / EXIF / 候选状态复制到新 location + backend。
    /// 用于 copy/move 完成后向 `output_index` 注册 dst 副本——dst 内容与 src 字节
    /// 等同，hash 直接复用避免对 dst 重新 stat + 读 4 KiB，也消除 `Info::open(dst)`
    /// 失败（NFS ESTALE / 防病毒抢占）→ dst 已写但未入索引 → 后续同 hash 源文件
    /// 被再次写入的语义漏洞。
    pub fn cloned_at(&self, new_loc: Location, new_backend: Arc<dyn Backend>) -> Self {
        let new_full_path: Utf8PathBuf = match &new_loc {
            Location::Local(p) => p.clone(),
            other => other.display().into(),
        };
        let lazy_snapshot = *self.lazy.lock();
        Self {
            fast_hash: self.fast_hash,
            full_path: new_full_path,
            size: self.size,
            location: new_loc,
            backend: new_backend,
            exif: self.exif.clone(),
            extra_candidates: self.extra_candidates.clone(),
            category: self.category.clone(),
            lazy: Mutex::new(lazy_snapshot),
            meta: self.meta.clone(),
        }
    }

    /// 计算创建时间。走 docs/media-time-detection.md 的 P0→P4 优先级判定：
    /// 把 EXIF/视频容器字段（P0/P1）、文件名启发式（P2）、外部注入的 sidecar
    /// 候选（P3，见 [`Self::add_candidates`]）、文件 mtime（P4）一起喂给
    /// `media_time::resolve`，decision 时间若小于 `valid_threshold_secs`
    ///（配置层的"软阈值"）则回退到 fs 兜底。
    /// `valid_threshold_secs` 与 `default_offset`（naive 时间的解释时区）由
    /// Use Case 层从配置读入；Entity 不直接依赖配置加载。
    pub fn create_time(
        &self,
        valid_threshold_secs: u64,
        default_offset: FixedOffset,
    ) -> std::time::SystemTime {
        time::create_time(self, valid_threshold_secs, default_offset)
    }

    /// 完整拍摄时间决策（P0→P4 优先级 + 冲突列表），供归档决策与 `verify` 对账
    /// 共用。返回 resolve 原样 decision，不做软阈值过滤（调用方按需解释）。
    pub fn media_time_decision(
        &self,
        default_offset: FixedOffset,
    ) -> Option<media_time::MediaTimeDecision> {
        time::media_time_decision(self, default_offset)
    }

    pub fn is_media(&self) -> bool {
        self.exif.as_ref().is_some_and(exif::Exif::is_media)
    }

    /// 是否文档族（PDF / OOXML / CFB / iWork / ODF / RTF / EPUB / 思维导图 /
    /// 纯文本）；`copy-doc`/`move-doc` 的 `do_copy` 落盘过滤用。
    pub fn is_office(&self) -> bool {
        self.exif.as_ref().is_some_and(exif::Exif::is_office)
    }

    /// 注入文档内容类目（`Index::classify_documents` 并行写回）。
    pub fn set_category(&mut self, category: String) {
        self.category = Some(category);
    }

    /// 文档内容类目；None = 未分类，`{category}` 渲染时兜底 uncategorized。
    pub fn category_ref(&self) -> Option<&str> {
        self.category.as_deref()
    }
}

// `Info::open` 的 boundary check helper：拒 "目录 / 0 字节"。
// 语义由 info_open_rejects_directory_* / info_open_rejects_empty_* 单元测试断言。
fn ensure_hashable(meta: &crate::entities::backend::Metadata, loc: &Location) -> io::Result<()> {
    if meta.kind != EntryKind::File {
        return Err(io::Error::other(format!(
            "{} is a directory",
            loc.display()
        )));
    }
    if meta.size == 0 {
        return Err(io::Error::other(format!("{} is empty", loc.display())));
    }
    Ok(())
}

impl PartialEq for Info {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}
