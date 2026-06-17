//! OOXML 字节扫描单测：helper 微测覆盖 `extract_dates` / `scan_element_text` /
//! `parse_iso8601_to_epoch` 分支。整 fn `parse(reader, mime)` 由 fixture 集成测试覆盖
//! （subprocess 跑 docx fixture 让 bin instance 命中 happy path）。

use super::*;

#[test]
fn extract_dates_happy_path() {
    let xml = br#"<?xml version="1.0"?>
<cp:coreProperties xmlns:cp="..." xmlns:dcterms="http://purl.org/dc/terms/">
  <dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2018-01-01T12:00:00Z</dcterms:modified>
</cp:coreProperties>"#;
    let (c, m) = extract_dates(xml);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_missing_modified_returns_zero() {
    let xml = b"<dcterms:created>2017-02-14T10:30:00Z</dcterms:created>";
    let (c, m) = extract_dates(xml);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_no_tags_returns_zeros() {
    assert_eq!(extract_dates(b"<no relevant tags here />"), (0, 0));
}

// ============= scan_element_text 边界 =============

#[test]
fn scan_element_text_with_attributes() {
    let buf = br#"<dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>"#;
    let text = scan_element_text(buf, b"<dcterms:created", b"</dcterms:created>").unwrap();
    assert_eq!(text, "2017-02-14T10:30:00Z");
}

#[test]
fn scan_element_text_no_open_tag_returns_none() {
    assert!(scan_element_text(b"<other>x</other>", b"<dcterms:created", b"</dcterms:created>").is_none());
}

#[test]
fn scan_element_text_no_gt_after_open_returns_none() {
    // open_tag 找到但后续无 `>` —— text 永不开始。
    let buf = b"<dcterms:created xsi:type=\"";
    assert!(scan_element_text(buf, b"<dcterms:created", b"</dcterms:created>").is_none());
}

#[test]
fn scan_element_text_no_close_tag_returns_none() {
    let buf = b"<dcterms:created>text but no close";
    assert!(scan_element_text(buf, b"<dcterms:created", b"</dcterms:created>").is_none());
}

#[test]
fn scan_element_text_non_utf8_text_returns_none() {
    let mut buf: Vec<u8> = b"<dcterms:created>".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</dcterms:created>");
    assert!(scan_element_text(&buf, b"<dcterms:created", b"</dcterms:created>").is_none());
}

// ============= parse_iso8601_to_epoch 边界 =============

#[test]
fn parse_iso8601_z_utc() {
    assert_eq!(
        parse_iso8601_to_epoch("2017-02-14T10:30:00Z"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_with_offset() {
    // 2017-02-14T18:30:00+08:00 = 10:30 UTC
    assert_eq!(
        parse_iso8601_to_epoch("2017-02-14T18:30:00+08:00"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_with_leading_whitespace_trims() {
    assert_eq!(
        parse_iso8601_to_epoch("  2017-02-14T10:30:00Z  "),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_invalid_format_returns_none() {
    assert!(parse_iso8601_to_epoch("not a date").is_none());
}

#[test]
fn parse_iso8601_pre_epoch_returns_none() {
    assert!(parse_iso8601_to_epoch("1969-12-31T00:00:00Z").is_none());
}

// ============= find_byte / find_subslice =============

#[test]
fn find_byte_found_and_not_found() {
    assert_eq!(find_byte(b"abc>def", b'>'), Some(3));
    assert!(find_byte(b"abcdef", b'>').is_none());
}

#[test]
fn find_subslice_found_and_not_found() {
    assert_eq!(find_subslice(b"xyzabc", b"abc"), Some(3));
    assert!(find_subslice(b"xyz", b"abc").is_none());
}
