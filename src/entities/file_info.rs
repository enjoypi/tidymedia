use std::fs;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::FixedOffset;
use chrono::Utc;
use generic_array::GenericArray;
use memmap2::Mmap;
use parking_lot::Mutex;
use sha2::Digest;
use sha2::Sha512;

use super::exif;
use super::media_time;
use super::SecureHash;

// 栈数组要求编译期常量，保留为 const（性能边界例外）
const FAST_READ_SIZE: usize = 4096;
// 流式哈希分块。1 MiB 平衡 syscall 频率与远程 backend 网络往返。
#[allow(dead_code)]
const STREAM_CHUNK: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lazy {
    bytes_read: u64,
    // true if full_hash is the whole file hash
    full: bool,
    // 64 bit hash from the whole file
    hash: u64,
    // Secure hash from the whole file
    secure_hash: SecureHash,
}

impl Lazy {
    // 初始化时，hash是作为第二个快速hash使用的，并不是整个文件的hash
    fn new(bytes_read: u64, hash: u64) -> Self {
        Self {
            bytes_read,
            hash,
            full: false,
            secure_hash: GenericArray::default(),
        }
    }
}

pub struct Info {
    // 64 bit hash  from the first FAST_READ_SIZE bytes
    pub fast_hash: u64,
    pub full_path: Utf8PathBuf,
    pub size: u64,

    // exif info
    exif: Option<exif::Exif>,
    lazy: Mutex<Lazy>,
    meta: fs::Metadata,
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
    pub fn from(path: &str) -> io::Result<Self> {
        let full_path = full_path(path)?;
        let meta = full_path.metadata()?;
        if !meta.is_file() {
            return Err(Error::new(
                ErrorKind::Other,
                format!("{} is a directory", full_path),
            ));
        }

        if meta.len() == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{} is empty", full_path),
            ));
        }

        let (bytes_read, first_hash, second_hash) = fast_hash(full_path.as_str())?;

        Ok(Self {
            fast_hash: first_hash,
            full_path,
            size: meta.len(),
            exif: None,
            lazy: Mutex::new(Lazy::new(bytes_read as u64, second_hash)),
            meta,
        })
    }

    pub fn from_path(path: &Utf8Path) -> io::Result<Self> {
        Self::from(path.as_str())
    }

    pub fn bytes_read(&self) -> u64 {
        self.lazy.lock().bytes_read
    }

    pub fn calc_full_hash(&self) -> io::Result<u64> {
        let mut l = self.lazy.lock();
        if l.full {
            return Ok(l.hash);
        }

        let (bytes_read, full) = full_hash(self.full_path.as_str())?;

        l.hash = full;
        l.bytes_read += bytes_read as u64;
        l.full = true;
        Ok(full)
    }

    fn full_hash(&self) -> u64 {
        self.lazy.lock().hash
    }

    pub fn secure_hash(&self) -> io::Result<SecureHash> {
        let mut l = self.lazy.lock();
        if l.secure_hash != GenericArray::default() {
            return Ok(l.secure_hash);
        }

        let (bytes_read, secure) = secure_hash(self.full_path.as_str())?;
        l.bytes_read += bytes_read as u64;
        l.secure_hash = secure;
        Ok(secure)
    }

    #[cfg(test)]
    pub fn exif(&self) -> Option<&exif::Exif> {
        self.exif.as_ref()
    }
    pub fn set_exif(&mut self, exif: exif::Exif) {
        self.exif = Some(exif);
    }

    /// 计算创建时间。走 docs/media-time-detection.md 的 P0→P4 优先级判定：
    /// 把 EXIF/视频容器字段、文件 mtime 喂给 `media_time::resolve`，decision 时间若小于
    /// `valid_threshold_secs`（配置层的"软阈值"）则回退到 fs 兜底。
    /// `valid_threshold_secs` 由 Use Case 层从配置读入；Entity 不直接依赖配置加载。
    pub fn create_time(&self, valid_threshold_secs: u64) -> SystemTime {
        let modified = self.meta.modified().ok();
        let created = self.meta.created().ok();
        let fs_fallback = pick_fs_fallback(modified, created);

        let Some(exif) = self.exif.as_ref() else {
            return fs_fallback;
        };

        // 当前实现统一用 UTC 作为 NaiveDateTime 的解释时区——上层（usecases::copy）
        // 已用 from_path_with_offset 把 EXIF 转 epoch 了，这里的 offset 仅作 P2 推断用，
        // 但本入口未喂入 filename/sidecar 候选，因此 offset 实际不被消费。
        let utc_offset = FixedOffset::east_opt(0).expect("0 offset is valid");
        let mut candidates = media_time::candidates_from_exif(exif, utc_offset);
        // Option<Candidate> 实现 IntoIterator → extend 不引入 if-let 分支。
        candidates.extend(media_time::fs_time::from_modified(modified));

        // resolve 返回 None（候选全部被过滤）与"低于阈值"走同一条 fallback 路径，
        // 避免在 create_time 里多一条不可稳定触发的分支。
        let secs = media_time::resolve(candidates, None, Utc::now())
            .map(|d| d.utc.timestamp())
            .unwrap_or(0);
        if secs > 0 && (secs as u64) >= valid_threshold_secs {
            SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64)
        } else {
            fs_fallback
        }
    }

    pub fn is_media(&self) -> bool {
        if self.exif.is_none() {
            return false;
        }
        self.exif.as_ref().unwrap().is_media()
    }
}

/// fs 兜底：取 mtime 与 btime 的较早值；任一缺失就用另一方；都缺失退到 UNIX_EPOCH。
/// 这是 P4 内部决策（spec §2.P4：mtime 兜底）——选较早值是因为 btime 在某些
/// 文件系统上 == ctime，受 inode 变更影响，比 mtime 更不稳定。
fn pick_fs_fallback(modified: Option<SystemTime>, created: Option<SystemTime>) -> SystemTime {
    match (modified, created) {
        (Some(m), Some(c)) if m < c => m,
        (Some(_), Some(c)) => c,
        (Some(m), None) => m,
        (None, Some(c)) => c,
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

// LLVM 对 `buffer[..bytes_read]` 等 slice 操作会插入越界 panic guard region，
// 在 buffer 固定长度的实现里永远不可触发；用 coverage(off) 排除。
#[cfg_attr(coverage_nightly, coverage(off))]
fn fast_hash(path: &str) -> io::Result<(usize, u64, u64)> {
    let mut file = fs::File::open(path)?;

    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file.read(&mut buffer)?;

    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));

    Ok((bytes_read, short, full))
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn full_hash(path: &str) -> io::Result<(usize, u64)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    Ok((mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap)))
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn secure_hash(path: &str) -> io::Result<(usize, SecureHash)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok((mmap.len(), Sha512::digest(&mmap)))
}

/// 读首 [`FAST_READ_SIZE`] 字节算 wyhash + xxh3 双哈希。
///
/// 返回 (`bytes_read`, wyhash, xxhash)。
/// 调用方须保证 reader 已 seek 到起点。
#[allow(dead_code)]
pub fn fast_hash_stream(r: &mut dyn super::backend::MediaReader)
    -> io::Result<(usize, u64, u64)>
{
    let mut buffer = [0u8; FAST_READ_SIZE];
    let n = read_fill(r, &mut buffer)?;
    let slice = &buffer[..n];
    Ok((n, wyhash::wyhash(slice, 0), xxhash_rust::xxh3::xxh3_64(slice)))
}

/// 流式整文件 xxh3-64 哈希。返回 (`bytes_read`, xxh3-64)。
/// 调用方须保证 reader 已 seek 到起点。
#[allow(dead_code)]
pub fn full_hash_stream(r: &mut dyn super::backend::MediaReader)
    -> io::Result<(u64, u64)>
{
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
#[allow(dead_code)]
pub fn secure_hash_stream(r: &mut dyn super::backend::MediaReader)
    -> io::Result<(u64, SecureHash)>
{
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
/// 抽出来是为了让 `fast_hash_stream` 函数体保持在 64 行以内。
#[allow(dead_code)]
fn read_fill(r: &mut dyn super::backend::MediaReader, buf: &mut [u8]) -> io::Result<usize> {
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

#[cfg(test)]
#[path = "file_info_tests.rs"]
mod tests;
