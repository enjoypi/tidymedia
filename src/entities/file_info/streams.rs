//! 流式哈希：fast(前 4 KiB 双哈希) / full(xxh3) / secure(SHA-512) 与 `read_fill` 复用。

use std::io;

use sha2::Digest;
use sha2::Sha512;

use crate::entities::SecureHash;
use crate::entities::backend::MediaReader;

// 栈数组要求编译期常量，保留为 const（性能边界例外）
pub(super) const FAST_READ_SIZE: usize = 4096;
// 流式哈希分块。1 MiB 平衡 syscall 频率与远程 backend 网络往返。
const STREAM_CHUNK: usize = 1 << 20;

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
pub(super) fn fast_hash(path: &str) -> io::Result<(usize, u64, u64)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file.read(&mut buffer)?;
    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));
    Ok((bytes_read, short, full))
}

#[cfg(test)]
pub(super) fn full_hash(path: &str) -> io::Result<(usize, u64)> {
    let file = std::fs::File::open(path)?;
    // SAFETY: file 句柄仍持有；测试用辅助，运行期外部进程不会并发改写。
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok((mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap)))
}

#[cfg(test)]
pub(super) fn secure_hash(path: &str) -> io::Result<(usize, SecureHash)> {
    let file = std::fs::File::open(path)?;
    // SAFETY: file 句柄仍持有；测试用辅助，运行期外部进程不会并发改写。
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok((mmap.len(), Sha512::digest(&mmap)))
}
