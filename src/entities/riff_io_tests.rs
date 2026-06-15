use std::io::Cursor;

use super::parse_avi_exif;
use super::tests_common::FailSeek;
use super::tests_common::avif_with_make;
use super::tests_common::chunk;
use super::tests_common::list;
use super::tests_common::parse;
use super::tests_common::riff_avi;
use super::{FOURCC_HDRL, FOURCC_LIST, FOURCC_STRD, FOURCC_STRL};

// ── IO 错误注入：截断与 seek 失败 ──

#[test]
fn list_type_truncated_returns_none() {
    // LIST 头完整但 list type 4 字节截断 → read_exact Err。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(FOURCC_LIST);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.push(b'h'); // type 只有 1 字节
    assert_eq!(parse(&bytes), None);
}

#[test]
fn hdrl_body_truncated_returns_none() {
    // hdrl 声称 100 字节 body 但流只剩 10 字节 → read_exact Err。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(FOURCC_LIST);
    bytes.extend_from_slice(&104u32.to_le_bytes());
    bytes.extend_from_slice(FOURCC_HDRL);
    bytes.extend_from_slice(&[0u8; 10]);
    assert_eq!(parse(&bytes), None);
}

#[test]
fn seek_failure_on_plain_chunk_returns_none() {
    let mut chunks = chunk(*b"JUNK", &[0u8; 2]);
    chunks.extend_from_slice(&list(
        *FOURCC_HDRL,
        &chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")),
    ));
    let mut r = FailSeek(Cursor::new(riff_avi(&chunks)));
    assert_eq!(parse_avi_exif(&mut r), None);
}

#[test]
fn seek_failure_on_non_hdrl_list_returns_none() {
    let mut chunks = list(*b"INFO", &[0u8; 4]);
    chunks.extend_from_slice(&list(
        *FOURCC_HDRL,
        &chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")),
    ));
    let mut r = FailSeek(Cursor::new(riff_avi(&chunks)));
    assert_eq!(parse_avi_exif(&mut r), None);
}

#[test]
fn strl_payload_beyond_buffer_returns_none() {
    // strl type 字段在界内但声称的 body 越界 → get(body+4..body+size) None。
    let mut bad = Vec::new();
    bad.extend_from_slice(FOURCC_LIST);
    bad.extend_from_slice(&100u32.to_le_bytes());
    bad.extend_from_slice(FOURCC_STRL); // type 可读，其后不足 96 字节
    bad.extend_from_slice(&[0u8; 8]);
    assert_eq!(parse(&riff_avi(&list(*FOURCC_HDRL, &bad))), None);
}
