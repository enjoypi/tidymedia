//! FreeMind/FreePlane `.mm` 单测。

use super::*;

#[test]
fn extract_dates_happy_path() {
    // CREATED=1487068200000 (2017-02-14T10:30:00Z), MODIFIED=1514808000000 (2018-01-01T12:00:00Z)
    let mm = br#"<?xml version="1.0"?>
<map version="1.0.1">
<node CREATED="1487068200000" MODIFIED="1514808000000" TEXT="root">
  <node CREATED="1500000000000" MODIFIED="1500000000000" TEXT="child"/>
</node>
</map>"#;
    let (c, m) = extract_dates(mm);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_no_node_returns_zeros() {
    assert_eq!(extract_dates(b"<map></map>"), (0, 0));
}

#[test]
fn extract_dates_missing_modified_returns_zero() {
    let mm = br#"<node CREATED="1487068200000" TEXT="root"/>"#;
    let (c, m) = extract_dates(mm);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_no_gt_after_node_uses_buf_end() {
    // `<node` 后无 `>` → attr_end = after.len()，仍能扫描 attrs。
    let mm = br#"<node CREATED="1487068200000""#;
    let (c, _) = extract_dates(mm);
    assert_eq!(c, 1_487_068_200);
}

// ============= scan_quoted_u64 =============

#[test]
fn scan_quoted_u64_basic() {
    assert_eq!(
        scan_quoted_u64(br#"CREATED="1487068200000""#, b"CREATED=\""),
        Some(1_487_068_200_000)
    );
}

#[test]
fn scan_quoted_u64_no_key_returns_none() {
    assert!(scan_quoted_u64(br"OTHER=", b"CREATED=\"").is_none());
}

#[test]
fn scan_quoted_u64_no_closing_quote_returns_none() {
    assert!(scan_quoted_u64(br#"CREATED="1487"#, b"CREATED=\"").is_none());
}

#[test]
fn scan_quoted_u64_non_digit_returns_none() {
    assert!(scan_quoted_u64(br#"CREATED="not_a_number""#, b"CREATED=\"").is_none());
}

#[test]
fn scan_quoted_u64_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"CREATED=\"".to_vec();
    buf.push(0xff);
    buf.push(b'"');
    assert!(scan_quoted_u64(&buf, b"CREATED=\"").is_none());
}

// ============= millis_to_secs =============

#[test]
fn millis_to_secs_modern() {
    assert_eq!(millis_to_secs(1_487_068_200_000), Some(1_487_068_200));
}

#[test]
fn millis_to_secs_pre_first_day_returns_none() {
    // < 1970-01-02 视为无效。
    assert!(millis_to_secs(60_000).is_none());
}

// ============= find_byte / find_subslice =============

#[test]
fn find_byte_found_and_not_found() {
    assert_eq!(find_byte(b"abc>def", b'>'), Some(3));
    assert!(find_byte(b"abcdef", b'>').is_none());
}

#[test]
fn find_subslice_found_and_not_found() {
    assert_eq!(find_subslice(b"hello world", b"world"), Some(6));
    assert!(find_subslice(b"hello", b"world").is_none());
}

// ============= collect_text_attrs（extract_text 业务） =============

#[test]
fn collect_text_attrs_multiple_nodes() {
    let mut out = String::new();
    collect_text_attrs(
        r#"<node TEXT="根主题"><node TEXT="子主题"/></node>"#.as_bytes(),
        &mut out,
        64,
    );
    assert_eq!(out, "根主题 子主题");
}

#[test]
fn collect_text_attrs_empty_value_skipped() {
    let mut out = String::new();
    collect_text_attrs(br#"<node TEXT=""/><node TEXT="x"/>"#, &mut out, 64);
    assert_eq!(out, "x");
}

#[test]
fn collect_text_attrs_unterminated_quote_stops() {
    let mut out = String::new();
    collect_text_attrs(br#"<node TEXT="never closed"#, &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn collect_text_attrs_budget_truncates() {
    let mut out = String::new();
    collect_text_attrs(br#"<node TEXT="abcdefgh"/>"#, &mut out, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn collect_text_attrs_no_attr_yields_empty() {
    let mut out = String::new();
    collect_text_attrs(b"<map version=\"1.0\"/>", &mut out, 64);
    assert_eq!(out, "");
}
