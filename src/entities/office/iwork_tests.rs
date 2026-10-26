#![allow(
    clippy::duration_suboptimal_units,
    reason = "Unix epoch 秒数语义直观，from_secs 是测试约定"
)]

use std::io::Cursor;
use std::time::{Duration, UNIX_EPOCH};

use crate::entities::office::tests_common::{corrupt_zip_entry, make_zip};

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
fn parse_happy_path_extracts_dates() {
    let plist = build_plist_with_dates(Some(1_487_068_200), Some(1_514_808_000));
    let z = make_zip(&[("Metadata/Properties.plist", plist.as_slice())]);
    let mut r = Cursor::new(z);
    assert_eq!(
        parse(&mut r, "application/vnd.apple.pages"),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn parse_missing_plist_returns_zeros() {
    let z = make_zip(&[("index.xml", b"<x/>".as_slice())]);
    let mut r = Cursor::new(z);
    assert_eq!(parse(&mut r, "application/vnd.apple.pages"), (0, 0));
}

#[test]
fn parse_corrupt_plist_read_error_returns_zeros() {
    let plist = build_plist_with_dates(Some(1_487_068_200), None);
    let mut z = make_zip(&[("Metadata/Properties.plist", plist.as_slice())]);
    corrupt_zip_entry(&mut z, b"createdDate");
    let mut r = Cursor::new(z);
    assert_eq!(parse(&mut r, "application/vnd.apple.pages"), (0, 0));
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
    let value = plist::Value::Array(vec![plist::Value::Integer(1.into())]);
    let mut buf = Vec::new();
    value.to_writer_binary(Cursor::new(&mut buf)).unwrap();
    assert_eq!(extract_dates_from_plist(&buf), (0, 0));
}

#[test]
fn extract_dates_non_date_value_returns_zero_for_that_field() {
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

#[test]
fn systemtime_to_epoch_post_epoch() {
    let t = UNIX_EPOCH + Duration::from_secs(1_487_068_200);
    assert_eq!(systemtime_to_epoch(t), Some(1_487_068_200));
}

#[test]
fn systemtime_to_epoch_pre_unix_epoch_returns_none() {
    let t = UNIX_EPOCH - Duration::from_secs(100);
    assert!(systemtime_to_epoch(t).is_none());
}
