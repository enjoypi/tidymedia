use std::fs;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use generic_array::GenericArray;
use memmap2::Mmap;
use parking_lot::Mutex;
use sha2::Digest;
use sha2::Sha512;

use super::exif;
use super::SecureHash;

// 栈数组要求编译期常量，保留为 const（性能边界例外）
const FAST_READ_SIZE: usize = 4096;

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

    /// 计算创建时间。EXIF 时间戳若小于 `valid_threshold_secs` 视为无效，回退到文件 mtime。
    /// 阈值由调用方（Use Case 层）从配置读取并传入——Entity 不直接依赖配置加载。
    // ext4 等 Linux 文件系统可能不报 btime；mtime 极少不可用。这里用 UNIX_EPOCH/兄弟字段兜底，
    // 让函数对 fs 差异具有鲁棒性，同时消除调用链中无法触发的 IO Err 分支。
    pub fn create_time(&self, valid_threshold_secs: u64) -> SystemTime {
        let file_modify_time = self.meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let file_create_time = self.meta.created().unwrap_or(file_modify_time);

        let real_create_time = if file_modify_time < file_create_time {
            file_modify_time
        } else {
            file_create_time
        };

        if self.exif.is_none() {
            return real_create_time;
        }
        let exif = self.exif.as_ref().unwrap();

        let t = exif.media_create_date();
        // ">=" 与"小于此值视为不可信"的注释语义一致：等于阈值的边界值采纳。
        if t >= valid_threshold_secs {
            SystemTime::UNIX_EPOCH + Duration::from_secs(t)
        } else {
            real_create_time
        }
    }

    pub fn is_media(&self) -> bool {
        if self.exif.is_none() {
            return false;
        }
        self.exif.as_ref().unwrap().is_media()
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

#[cfg(test)]
#[path = "file_info_tests.rs"]
mod tests;
