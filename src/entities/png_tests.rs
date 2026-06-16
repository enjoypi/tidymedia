use std::io;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;

use super::CHUNK_TYPE_LEN;
use super::PNG_SIGNATURE;
use super::parse_png_exif;

/// `Read` 走 inner `Cursor`，`Seek` 恒返 Err。用于触发 `png.rs` L65
/// `r.seek(...).ok()?` 的 None 分支（`Cursor::seek` 永不失败）。
#[derive(Debug)]
struct FailSeek(Cursor<Vec<u8>>);

impl Read for FailSeek {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for FailSeek {
    fn seek(&mut self, _: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::other("seek refused for test"))
    }
}

// ---------- 字节构造 helper ----------

fn png_chunk(chunk_type: [u8; CHUNK_TYPE_LEN], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    let len = u32::try_from(data.len()).expect("test chunk fits u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&chunk_type);
    out.extend_from_slice(data);
    // CRC 占位（解析器不验证 CRC，按 YAGNI 填 0）
    out.extend_from_slice(&[0u8; 4]);
    out
}

/// 最小合法 eXIf payload（II / 0x002A magic / IFD0 offset=8 / 1 entry: Make="Cam\0\0"）
fn minimal_exif_payload() -> Vec<u8> {
    // 布局：header 8 + IFD0 count 2 + entry 12 + next-IFD 4 = 26；Make data 起 26
    let mut payload = Vec::new();
    payload.extend_from_slice(b"II");
    payload.extend_from_slice(&0x002A_u16.to_le_bytes());
    payload.extend_from_slice(&8_u32.to_le_bytes());
    payload.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    payload.extend_from_slice(&0x010f_u16.to_le_bytes()); // Make tag
    payload.extend_from_slice(&2_u16.to_le_bytes()); // ASCII type
    payload.extend_from_slice(&5_u32.to_le_bytes()); // cnt
    payload.extend_from_slice(&26_u32.to_le_bytes()); // offset
    payload.extend_from_slice(&0_u32.to_le_bytes()); // next IFD
    payload.extend_from_slice(b"Cam\0\0"); // 5 bytes
    payload
}

fn build_png_with_exif() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    buf.extend_from_slice(&png_chunk(*b"eXIf", &minimal_exif_payload()));
    buf.extend_from_slice(&png_chunk(*b"IEND", &[]));
    buf
}

// ---------- 正常路径 ----------

#[test]
fn parse_png_exif_reads_minimal_make() {
    let buf = build_png_with_exif();
    let mut cur = Cursor::new(buf);
    let ifd = parse_png_exif(&mut cur).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

#[test]
fn parse_png_exif_skips_non_target_chunks_before_exif() {
    // IHDR + IDAT + eXIf + IEND：IDAT 必须被 seek 跳过
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    buf.extend_from_slice(&png_chunk(*b"IDAT", &[0u8; 32]));
    buf.extend_from_slice(&png_chunk(*b"eXIf", &minimal_exif_payload()));
    buf.extend_from_slice(&png_chunk(*b"IEND", &[]));
    let mut cur = Cursor::new(buf);
    let ifd = parse_png_exif(&mut cur).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

// ---------- 拒绝路径 ----------

#[test]
fn parse_png_exif_rejects_non_png() {
    let mut cur = Cursor::new(b"NOT-A-PNG-FILE".to_vec());
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_rejects_truncated_signature() {
    let mut cur = Cursor::new(b"\x89PNG".to_vec());
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_iend_before_exif() {
    // IHDR → IEND，无 eXIf
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    buf.extend_from_slice(&png_chunk(*b"IEND", &[]));
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_exif_chunk_oversize() {
    // 仅 IHDR + 一个声明长度超 MAX_CHUNK_BYTES 的 eXIf chunk header
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    // 手写 eXIf chunk header：len=2GiB，无 data（解析在 oversize guard 前就返）
    buf.extend_from_slice(&(2_u32 << 30).to_be_bytes());
    buf.extend_from_slice(b"eXIf");
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_exif_payload_truncated() {
    // 声明 eXIf len=100 但仅给 10 字节 → read_exact 失败
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    buf.extend_from_slice(&100_u32.to_be_bytes());
    buf.extend_from_slice(b"eXIf");
    buf.extend_from_slice(&[0u8; 10]);
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_exif_payload_invalid_tiff() {
    // eXIf chunk 存在但 payload 不是合法 TIFF header → parse_tiff 返 None
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IHDR", &[0u8; 13]));
    buf.extend_from_slice(&png_chunk(*b"eXIf", b"NOT-A-TIFF-HEADER"));
    buf.extend_from_slice(&png_chunk(*b"IEND", &[]));
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_skip_seek_fails() {
    // 声明非目标 chunk len=1000 但 reader 长度只够到 chunk header → seek 越界失败
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&1000_u32.to_be_bytes());
    buf.extend_from_slice(b"IDAT");
    // 不给 data + CRC
    // Cursor::seek SeekFrom::Current 越界本身不报错，但下一 read_exact 会失败
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_chunk_limit_reached() {
    // 64 个非目标空 chunk 后才放 eXIf → 命中 MAX_CHUNKS 上限直接 None
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    for _ in 0..70 {
        buf.extend_from_slice(&png_chunk(*b"IDAT", &[]));
    }
    buf.extend_from_slice(&png_chunk(*b"eXIf", &minimal_exif_payload()));
    buf.extend_from_slice(&png_chunk(*b"IEND", &[]));
    let mut cur = Cursor::new(buf);
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_chunk_header_truncated() {
    // signature 后什么都没有 → read_exact chunk header 失败
    let mut cur = Cursor::new(PNG_SIGNATURE.to_vec());
    assert!(parse_png_exif(&mut cur).is_none());
}

#[test]
fn parse_png_exif_returns_none_when_skip_seek_returns_err() {
    // FailSeek 让 r.seek(...).ok()? 在 IDAT 跳过时返 None 触发 png.rs:65 None 分支。
    let mut buf = Vec::new();
    buf.extend_from_slice(PNG_SIGNATURE);
    buf.extend_from_slice(&png_chunk(*b"IDAT", &[0u8; 16]));
    buf.extend_from_slice(&png_chunk(*b"eXIf", &minimal_exif_payload()));
    let mut reader = FailSeek(Cursor::new(buf));
    assert!(parse_png_exif(&mut reader).is_none());
}
