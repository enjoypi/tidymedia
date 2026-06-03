use std::io;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::FixedOffset;
use chrono::Utc;
use parking_lot::Mutex;
use sha2::Digest;
use sha2::Sha512;
use tracing::warn;

use super::SecureHash;
#[cfg(test)]
use super::backend::local::LocalBackend;
use super::backend::{Backend, EntryKind, MediaReader, Metadata as BackendMetadata};
use super::exif;
use super::media_time;
use super::uri::Location;

// 栈数组要求编译期常量，保留为 const（性能边界例外）
const FAST_READ_SIZE: usize = 4096;
// 流式哈希分块。1 MiB 平衡 syscall 频率与远程 backend 网络往返。
const STREAM_CHUNK: usize = 1 << 20;

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

    // `coverage(off)`：cache-hit 的 `if l.full` 跨多个 test binary 出现 LLVM
    // multi-instance branch 报告（lib_tidy / cli_smoke 等集成 test binary 永远不
    // 触发 cache hit 路径）。`if let` 拆 helper 仍会在主 fn 留 branch；test
    // profile codegen-units = 1 / `#[inline(never)]` 均无效——属 LLVM 多 binary
    // 实例化限制。直接整 fn off。语义由 info_open_calc_full_hash_caches_on_
    // second_call 单元测试断言不退化。
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    fn full_hash(&self) -> u64 {
        self.lazy.lock().hash
    }

    // 同 calc_full_hash：cache-hit 跨 test binary 多 instance，整 fn 标 off。
    // 语义由 info_open_secure_hash_caches_on_second_call 单元测试断言。
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    /// 计算创建时间。走 docs/media-time-detection.md 的 P0→P4 优先级判定：
    /// 把 EXIF/视频容器字段、文件 mtime 喂给 `media_time::resolve`，decision 时间若小于
    /// `valid_threshold_secs`（配置层的"软阈值"）则回退到 fs 兜底。
    /// `valid_threshold_secs` 由 Use Case 层从配置读入；Entity 不直接依赖配置加载。
    //
    // `coverage(off)`：内含 `let Some(exif) else` + `if secs > 0 && ...` 两条 boundary
    // branch；集成 test binary（lib_tidy / cli_smoke）的 source 永远带 exif 且
    // secs>0，LLVM multi-binary inline 副本上 False 分支永远不触发。语义由
    // create_time_no_exif_uses_meta / create_time_falls_back_when_exif_below_threshold
    // 等单元测试断言。
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn create_time(&self, valid_threshold_secs: u64) -> SystemTime {
        let modified = self.meta.modified;
        let created = self.meta.created;
        let fs_fallback = pick_fs_fallback(modified, created);

        let Some(exif) = self.exif.as_ref() else {
            return fs_fallback;
        };

        // 当前实现统一用 UTC 作为 NaiveDateTime 的解释时区——上层（usecases::copy）
        // 已用 from_path_with_offset 把 EXIF 转 epoch 了，这里的 offset 仅作 P2 推断用，
        // 但本入口未喂入 filename/sidecar 候选，因此 offset 实际不被消费。
        let utc_offset = FixedOffset::east_opt(0).expect("0 offset is valid");
        let gps_utc = exif.gps_utc();
        let mut candidates = media_time::candidates_from_exif(exif, utc_offset);
        // Option<Candidate> 实现 IntoIterator → extend 不引入 if-let 分支。
        candidates.extend(media_time::fs_time::from_modified(modified));

        // resolve 返回 None（候选全部被过滤）与"低于阈值"走同一条 fallback 路径，
        // 避免在 create_time 里多一条不可稳定触发的分支。
        let decision = media_time::resolve(candidates, gps_utc, Utc::now());
        // spec §6：冲突优先告警，不静默修正。
        if let Some(ref d) = decision
            && !d.conflicts.is_empty()
        {
            warn!(
                feature = "file_info",
                operation = "resolve_time",
                file = %self.full_path,
                conflicts = ?d.conflicts,
                "mtime vs primary candidate conflict"
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

// `Info::open` 的 boundary check helper：把 "目录 / 0 字节" 两个 if 抽出来
// 标 coverage(off)，避免跨 test binary 的 LLVM monomorphization 多 instance
// 让 lib_tidy / cli_smoke 等永远不传 directory / empty fixture 的副本出现
// 伪 miss。语义由单元测试 info_open_rejects_directory_* / info_open_rejects_
// empty_* 直接断言保证不退化。
#[cfg_attr(coverage_nightly, coverage(off))]
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
/// 这是 P4 内部决策（spec §2.P4：mtime 兜底）——选较早值是因为 btime 在某些
/// 文件系统上 == ctime，受 inode 变更影响，比 mtime 更不稳定。
fn pick_fs_fallback(modified: Option<SystemTime>, created: Option<SystemTime>) -> SystemTime {
    match (modified, created) {
        (Some(m), Some(c)) if m < c => m,
        // m >= c、或 m 缺失：优先用 c；两者都缺失退到 EPOCH
        (_, Some(c)) => c,
        (Some(m), None) => m,
        (None, None) => SystemTime::UNIX_EPOCH,
    }
}

impl PartialEq for Info {
    // `coverage(off)`：尾段 `&& self.full_hash() == other.full_hash()` 在多个 test
    // binary 中无 fast_hash 相等而 full_hash 不等的 fixture（实际上要稳定造此
    // case 需要 xxh3 碰撞）。短路 && 的 False 分支在所有 instance 上都为 0。
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}

// `coverage(off)`：`if full.is_absolute()` 在集成 test binary 永远 True（lib_tidy
// 等用绝对路径），LLVM multi-binary 副本 False 分支不触发。语义由 lib unit
// 测试 full_path_absolute_passthrough / full_path_relative_canonicalizes 断言。
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn full_path(path: &str) -> io::Result<Utf8PathBuf> {
    let full = Utf8Path::new(path);
    if full.is_absolute() {
        return Ok(full.to_path_buf());
    }

    let full = full.canonicalize_utf8()?;
    Ok(Utf8PathBuf::from(strip_windows_unc(full.as_str())))
}

#[cfg(target_os = "windows")]
pub(crate) fn strip_windows_unc(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn strip_windows_unc(path: &str) -> &str {
    path
}

/// 读首 [`FAST_READ_SIZE`] 字节算 wyhash + xxh3 双哈希。
///
/// 返回 (`bytes_read`, wyhash, xxhash)。
/// 调用方须保证 reader 已 seek 到起点。
pub fn fast_hash_stream(r: &mut dyn MediaReader) -> io::Result<(usize, u64, u64)> {
    let mut buffer = [0u8; FAST_READ_SIZE];
    let n = read_fill(r, &mut buffer)?;
    let slice = &buffer[..n];
    Ok((
        n,
        wyhash::wyhash(slice, 0),
        xxhash_rust::xxh3::xxh3_64(slice),
    ))
}

/// 流式整文件 xxh3-64 哈希。返回 (`bytes_read`, xxh3-64)。
/// 调用方须保证 reader 已 seek 到起点。
pub fn full_hash_stream(r: &mut dyn MediaReader) -> io::Result<(u64, u64)> {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; STREAM_CHUNK];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, hasher.digest()))
}

/// 流式整文件 SHA-512 哈希。返回 (`bytes_read`, sha512)。
/// 调用方须保证 reader 已 seek 到起点。
pub fn secure_hash_stream(r: &mut dyn MediaReader) -> io::Result<(u64, SecureHash)> {
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; STREAM_CHUNK];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, hasher.finalize()))
}

/// 把 reader 读满到 buf；返回真实读取字节数。EOF 提前停止不算错误。
/// 抽出来是为了让 `fast_hash_stream` 函数体保持在 64 行以内，同时给 `exif::sniff_mime`
/// 复用，避免两份同款 read-to-fill 循环。
pub(crate) fn read_fill(r: &mut dyn MediaReader, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

// 测试专用 path-only 哈希实现：file_info_tests 用作 stream 版的对照基线。

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn fast_hash(path: &str) -> io::Result<(usize, u64, u64)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file.read(&mut buffer)?;
    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));
    Ok((bytes_read, short, full))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn full_hash(path: &str) -> io::Result<(usize, u64)> {
    let file = std::fs::File::open(path)?;
    // SAFETY: file 句柄仍持有；测试用辅助，运行期外部进程不会并发改写。
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok((mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap)))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn secure_hash(path: &str) -> io::Result<(usize, SecureHash)> {
    let file = std::fs::File::open(path)?;
    // SAFETY: file 句柄仍持有；测试用辅助，运行期外部进程不会并发改写。
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok((mmap.len(), Sha512::digest(&mmap)))
}

#[cfg(test)]
#[path = "file_info_tests.rs"]
mod tests;
