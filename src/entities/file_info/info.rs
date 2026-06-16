//! `Info` 实体：按需哈希（fast/full/secure）缓存 + EXIF 持有 + 拍摄时间裁决入口。

use std::io;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::FixedOffset;
use chrono::TimeZone;
use chrono::Utc;
use parking_lot::Mutex;
use tracing::warn;

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
    ) -> SystemTime {
        let modified = self.meta.modified;
        let created = self.meta.created;
        let fs_fallback = pick_fs_fallback(modified, created);

        // P2 文件名中的 naive 时间按 default_offset（配置时区）解释，与 EXIF naive
        // 同口径——按 UTC 解释会让月末晚间拍摄的文件 +offset 后跨月归错桶；
        // P0/P1 的 epoch 已在 EXIF 解析层（from_path_with_offset）按配置时区转换完毕，
        // 这里的 offset 对其仅作候选元数据。
        let gps_utc = self.exif.as_ref().and_then(exif::Exif::gps_utc);
        // ModifyDate 不进候选，仅作多数派仲裁的 re-save 旁证；epoch 已在
        // EXIF 解析层按配置时区转换，0 = 缺失。
        let modify_date_utc = self
            .exif
            .as_ref()
            .map(exif::Exif::exif_modify_date)
            .filter(|&s| s > 0)
            .and_then(|s| Utc.timestamp_opt(s.cast_signed(), 0).single());
        let mut candidates = match self.exif.as_ref() {
            Some(exif) => media_time::candidates_from_exif(exif, default_offset),
            None => Vec::new(),
        };
        // P2：文件名启发式（IMG_/DSC_/Screenshot_/毫秒戳等）。
        candidates.extend(media_time::candidates_from_filename(
            Utf8Path::new(self.full_path.as_str()),
            default_offset,
        ));
        // P3：adapters 层发现并注入的 sidecar 候选（XMP / Google Takeout）。
        candidates.extend(self.extra_candidates.iter().copied());
        // P4。Option<Candidate> 实现 IntoIterator → extend 不引入 if-let 分支。
        candidates.extend(media_time::fs_time::from_modified(modified));

        // resolve 返回 None（候选全部被过滤）与"低于阈值"走同一条 fallback 路径，
        // 避免在 create_time 里多一条不可稳定触发的分支。
        let decision = media_time::resolve(candidates, gps_utc, modify_date_utc, Utc::now());
        // 冲突优先告警，不静默修正。
        if let Some(ref d) = decision
            && !d.conflicts.is_empty()
        {
            warn!(
                feature = "file_info",
                operation = "resolve_time",
                file = %self.full_path,
                conflicts = ?d.conflicts,
                "media time candidates conflict"
            );
        }
        let secs = decision.map_or(0, |d| d.utc.timestamp());
        if secs > 0 && secs.cast_unsigned() >= valid_threshold_secs {
            SystemTime::UNIX_EPOCH + Duration::from_secs(secs.cast_unsigned())
        } else {
            fs_fallback
        }
    }

    pub fn is_media(&self) -> bool {
        self.exif.as_ref().is_some_and(exif::Exif::is_media)
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

/// fs 兜底：取 mtime 与 btime 的较早值；任一缺失就用另一方；都缺失退到 `UNIX_EPOCH`。
/// 这是 P4 内部决策（mtime 兜底）——选较早值是因为 btime 在某些
/// 文件系统上 == ctime，受 inode 变更影响，比 mtime 更不稳定。
pub(super) fn pick_fs_fallback(
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
) -> SystemTime {
    match (modified, created) {
        (Some(m), Some(c)) if m < c => m,
        // m >= c、或 m 缺失：优先用 c；两者都缺失退到 EPOCH
        (_, Some(c)) => c,
        (Some(m), None) => m,
        (None, None) => SystemTime::UNIX_EPOCH,
    }
}

impl PartialEq for Info {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}
