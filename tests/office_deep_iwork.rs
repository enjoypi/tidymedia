//! iWork 容器 deep 测试：`iwork_parse` 的 zip 容器防御分支（zip 打开失败 / plist
//! entry 缺失 / 读 CRC 失败）+ `extract_dates_from_plist` 纯函数分支。unit helper
//! 不可见，plist 字典用 `plist` crate 合成（与 `iwork_tests.rs` 同法）。
#![allow(
    clippy::duration_suboptimal_units,
    clippy::single_char_pattern,
    reason = "Unix epoch 秒数语义直观，from_secs 是测试约定"
)]

use std::io::{Cursor, Write};
use std::time::{Duration, UNIX_EPOCH};

use tidymedia::{iwork_extract_dates_from_plist, iwork_parse};

const PROPERTIES_PLIST: &str = "Metadata/Properties.plist";
const KEY_CREATED: &str = "createdDate";
const KEY_MODIFIED: &str = "modifiedDate";

fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut c);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }
    c.into_inner()
}

fn flip_byte(buf: &mut [u8], needle: &[u8]) {
    let pos = buf
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("needle present");
    buf[pos] ^= 1;
}

fn build_plist_with_dates(created_secs: Option<u64>, modified_secs: Option<u64>) -> Vec<u8> {
    let mut dict = plist::Dictionary::new();
    if let Some(c) = created_secs {
        let t = UNIX_EPOCH + Duration::from_secs(c);
        dict.insert(KEY_CREATED.into(), plist::Value::Date(t.into()));
    }
    if let Some(m) = modified_secs {
        let t = UNIX_EPOCH + Duration::from_secs(m);
        dict.insert(KEY_MODIFIED.into(), plist::Value::Date(t.into()));
    }
    let mut buf = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_binary(Cursor::new(&mut buf))
        .expect("write binary plist");
    buf
}

fn parse_bytes(bytes: &[u8]) -> (u64, u64) {
    iwork_parse(&mut Cursor::new(bytes.to_vec()), "application/x-iwork-key")
}

// ============= iwork_parse 容器分支 =============

#[test]
fn parse_happy_path_reads_both_dates() {
    let plist = build_plist_with_dates(Some(1_487_068_200), Some(1_514_808_000));
    let zip = make_zip(&[(PROPERTIES_PLIST, &plist)]);
    assert_eq!(parse_bytes(&zip), (1_487_068_200, 1_514_808_000));
}

#[test]
fn parse_garbage_bytes_returns_zero() {
    assert_eq!(parse_bytes(b"not a zip at all"), (0, 0));
}

#[test]
fn parse_zip_missing_properties_plist_returns_zero() {
    let zip = make_zip(&[("Index/Document.iwa", b"snappy-bytes")]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_corrupt_plist_data_returns_zero() {
    let plist = build_plist_with_dates(Some(1_487_068_200), None);
    let mut zip = make_zip(&[(PROPERTIES_PLIST, &plist)]);
    flip_byte(&mut zip, b"bplist00");
    assert_eq!(parse_bytes(&zip), (0, 0));
}

// ============= extract_dates_from_plist 纯函数 =============

#[test]
fn extract_dates_only_created_returns_modified_zero() {
    let buf = build_plist_with_dates(Some(1_487_068_200), None);
    assert_eq!(iwork_extract_dates_from_plist(&buf), (1_487_068_200, 0));
}

#[test]
fn extract_dates_only_modified_returns_created_zero() {
    let buf = build_plist_with_dates(None, Some(1_514_808_000));
    assert_eq!(iwork_extract_dates_from_plist(&buf), (0, 1_514_808_000));
}

#[test]
fn extract_dates_empty_dict_returns_zeros() {
    let buf = build_plist_with_dates(None, None);
    assert_eq!(iwork_extract_dates_from_plist(&buf), (0, 0));
}

#[test]
fn extract_dates_invalid_plist_returns_zeros() {
    assert_eq!(iwork_extract_dates_from_plist(b"not a plist"), (0, 0));
}

#[test]
fn extract_dates_non_dict_root_returns_zeros() {
    let value = plist::Value::Array(vec![plist::Value::Integer(1.into())]);
    let mut buf = Vec::new();
    value.to_writer_binary(Cursor::new(&mut buf)).unwrap();
    assert_eq!(iwork_extract_dates_from_plist(&buf), (0, 0));
}

#[test]
fn extract_dates_non_date_value_returns_zero_for_that_field() {
    let mut dict = plist::Dictionary::new();
    dict.insert(
        KEY_CREATED.into(),
        plist::Value::String("not a date".into()),
    );
    let mut buf = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_binary(Cursor::new(&mut buf))
        .unwrap();
    assert_eq!(iwork_extract_dates_from_plist(&buf), (0, 0));
}
