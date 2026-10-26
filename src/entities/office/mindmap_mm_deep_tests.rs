use std::io::Cursor;

use super::{
    collect_text_attrs as mm_collect_text_attrs, extract_dates as mm_extract_dates,
    extract_text as mm_extract_text, parse as mm_parse,
};
use crate::entities::office::tests_common::FailReader;

#[test]
fn mm_parse_happy_and_no_node() {
    let mm = br#"<node CREATED="1487068200000" MODIFIED="1514808000000" TEXT="root"/>"#;
    let mut reader = Cursor::new(mm.to_vec());
    assert_eq!(
        mm_parse(&mut reader, "application/x-freemind"),
        (1_487_068_200, 1_514_808_000)
    );
    let mut empty = Cursor::new(b"<map/>".to_vec());
    assert_eq!(mm_parse(&mut empty, "application/x-freemind"), (0, 0));
    assert_eq!(mm_extract_dates(b"<map/>"), (0, 0));
}

#[test]
fn mm_parse_read_error_returns_zeros() {
    let mut reader = FailReader;
    assert_eq!(mm_parse(&mut reader, "application/x-freemind"), (0, 0));
}

#[test]
fn mm_extract_text_happy() {
    let mm = br#"<map><node TEXT="root"><node TEXT="child"/></node></map>"#;
    let mut reader = Cursor::new(mm.to_vec());
    assert_eq!(
        mm_extract_text(&mut reader, "application/x-freemind", 64),
        "root child"
    );
}

#[test]
fn mm_extract_text_read_error_returns_empty() {
    let mut reader = FailReader;
    assert_eq!(
        mm_extract_text(&mut reader, "application/x-freemind", 64),
        ""
    );
}

#[test]
fn mm_collect_text_attrs_basic() {
    let mut out = String::new();
    mm_collect_text_attrs(br#"<node TEXT="a"/><node TEXT="b"/>"#, &mut out, 64);
    assert_eq!(out, "a b");
}
