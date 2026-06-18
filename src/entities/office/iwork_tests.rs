//! iWork plist 字节扫描单测。用 plist crate write 在测试中合成 binary plist 字典，
//! 覆盖 `extract_dates_from_plist` / `systemtime_to_epoch` 分支。
//! 整 fn `parse(reader, mime)` `coverage(off)`，e2e 由 fixture 集成测试不覆盖
//! （iWork 文件结构因版本而异，单测 plist 二进制即足够）。
#![allow(
    clippy::duration_suboptimal_units,
    reason = "Unix epoch 秒数语义直观，from_secs 是测试约定"
)]

use std::io::Cursor;
use std::time::{Duration, UNIX_EPOCH};

use super::*;

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
    let value = plist::Value::Dictionary(dict);
    let mut buf = Vec::new();
    value
        .to_writer_binary(Cursor::new(&mut buf))
        .expect("write binary plist");
    buf
}

#[test]
fn extract_dates_happy_path() {
    let buf = build_plist_with_dates(Some(1_487_068_200), Some(1_514_808_000));
    assert_eq!(
        extract_dates_from_plist(&buf),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn extract_dates_only_created_returns_modified_zero() {
    let buf = build_plist_with_dates(Some(1_487_068_200), None);
    let (c, m) = extract_dates_from_plist(&buf);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_only_modified_returns_created_zero() {
    let buf = build_plist_with_dates(None, Some(1_514_808_000));
    let (c, m) = extract_dates_from_plist(&buf);
    assert_eq!(c, 0);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_empty_dict_returns_zeros() {
    let buf = build_plist_with_dates(None, None);
    assert_eq!(extract_dates_from_plist(&buf), (0, 0));
}

#[test]
fn extract_dates_invalid_plist_returns_zeros() {
    assert_eq!(extract_dates_from_plist(b"not a plist"), (0, 0));
}

#[test]
fn extract_dates_non_dict_root_returns_zeros() {
    // plist 顶层是数组而非字典 → as_dictionary 返 None。
    let value = plist::Value::Array(vec![plist::Value::Integer(1.into())]);
    let mut buf = Vec::new();
    value.to_writer_binary(Cursor::new(&mut buf)).unwrap();
    assert_eq!(extract_dates_from_plist(&buf), (0, 0));
}

#[test]
fn extract_dates_non_date_value_returns_zero_for_that_field() {
    // createdDate 是 String 而非 Date → as_date 返 None → 0。
    let mut dict = plist::Dictionary::new();
    dict.insert(
        KEY_CREATED.into(),
        plist::Value::String("not a date".into()),
    );
    let value = plist::Value::Dictionary(dict);
    let mut buf = Vec::new();
    value.to_writer_binary(Cursor::new(&mut buf)).unwrap();
    assert_eq!(extract_dates_from_plist(&buf), (0, 0));
}

// ============= systemtime_to_epoch =============

#[test]
fn systemtime_to_epoch_post_epoch() {
    let t = UNIX_EPOCH + Duration::from_secs(1_487_068_200);
    assert_eq!(systemtime_to_epoch(t), Some(1_487_068_200));
}

#[test]
fn systemtime_to_epoch_pre_unix_epoch_returns_none() {
    // SystemTime 早于 UNIX_EPOCH → duration_since Err → None。
    let t = UNIX_EPOCH - Duration::from_secs(100);
    assert!(systemtime_to_epoch(t).is_none());
}
