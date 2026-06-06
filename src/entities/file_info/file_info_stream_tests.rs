//! `Info` 的流式 hash + `Info::open` 远端 backend 集成 + `create_time` warn 测试。
//! 从 `file_info_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::fs;
use std::io;
use std::io::Cursor;
use std::io::Read;

use sha2::Digest;
use xxhash_rust::xxh3;

use super::super::test_common as common;

/// 单次 read 限量到 32 字节的 reader，触发流式哈希的多次循环回边。
#[derive(Debug)]
struct ChunkedReader {
    data: Vec<u8>,
    pos: usize,
}
impl ChunkedReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}
impl io::Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.data.len() - self.pos;
        let n = remaining.min(buf.len()).min(32);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
impl io::Seek for ChunkedReader {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "测试用小缓冲区，偏移量始终在 usize 范围内"
    )]
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match pos {
            io::SeekFrom::Start(p) => {
                self.pos = p as usize;
            }
            _ => return Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
        Ok(self.pos as u64)
    }
}

/// 始终返回 `io::Error` 的 reader，用于覆盖 `read_fill` / full / secure 的 `?` 错误分支。
#[derive(Debug)]
struct FailingReader;
impl io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}
impl io::Seek for FailingReader {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

fn whole_file_bytes(path: &str) -> Vec<u8> {
    let mut f = fs::File::open(path).unwrap();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();
    buf
}

#[test]
fn fast_hash_stream_matches_path_version_small() {
    let bytes = whole_file_bytes(common::DATA_SMALL);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_SMALL).unwrap();
    let mut r = Cursor::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_matches_path_version_large() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_handles_chunked_reader() {
    // ChunkedReader 单次最多 32 字节：read_fill 必须循环多次填满 buffer
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_LARGE).unwrap();
    let mut r = ChunkedReader::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_empty_reader() {
    // 立即 EOF：read_fill 第一次 read 返回 0，break 退出
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, w, x) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    assert_eq!(w, wyhash::wyhash(&[], 0));
    assert_eq!(x, xxh3::xxh3_64(&[]));
}

#[test]
fn fast_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    let err = super::fast_hash_stream(&mut r).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn full_hash_stream_matches_path_version() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_full) = super::full_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes.clone());
    let (sn, sh) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_full);
    assert_eq!(sn, bytes.len() as u64);
}

#[test]
fn full_hash_stream_chunked_matches() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_full) = super::full_hash(common::DATA_LARGE).unwrap();
    let mut r = ChunkedReader::new(bytes.clone());
    let (sn, sh) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_full);
    assert_eq!(sn, bytes.len() as u64);
}

#[test]
fn full_hash_stream_empty_reader() {
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, h) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    assert_eq!(h, xxh3::xxh3_64(&[]));
}

#[test]
fn full_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    assert_eq!(
        super::full_hash_stream(&mut r).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn secure_hash_stream_matches_path_version() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_secure) = super::secure_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes);
    let (_, sh) = super::secure_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_secure);
}

#[test]
fn secure_hash_stream_empty_reader() {
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, h) = super::secure_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    let expected = sha2::Sha512::digest(b"");
    assert_eq!(h, expected);
}

#[test]
fn secure_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    assert_eq!(
        super::secure_hash_stream(&mut r).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
