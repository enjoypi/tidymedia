//! ODF 字节扫描单测：覆盖 `extract_dates` / `scan_element_text` / `parse_odf_datetime`。

use super::*;

#[test]
fn extract_dates_happy_path() {
    let xml = b"<office:meta>
  <meta:creation-date>2017-02-14T10:30:00Z</meta:creation-date>
  <dc:date>2018-01-01T12:00:00Z</dc:date>
</office:meta>";
    let (c, m) = extract_dates(xml);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_naive_datetime_treats_as_utc() {
    let xml = b"<meta:creation-date>2017-02-14T10:30:00</meta:creation-date>";
    let (c, _) = extract_dates(xml);
    assert_eq!(c, 1_487_068_200);
}

#[test]
fn extract_dates_missing_both_returns_zeros() {
    assert_eq!(extract_dates(b"<no relevant tags />"), (0, 0));
}

// ============= parse_odf_datetime =============

#[test]
fn parse_odf_datetime_rfc3339_with_z() {
    assert_eq!(parse_odf_datetime("2017-02-14T10:30:00Z"), Some(1_487_068_200));
}

#[test]
fn parse_odf_datetime_rfc3339_with_offset() {
    assert_eq!(
        parse_odf_datetime("2017-02-14T18:30:00+08:00"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_odf_datetime_naive_falls_back_to_utc() {
    assert_eq!(parse_odf_datetime("2017-02-14T10:30:00"), Some(1_487_068_200));
}

#[test]
fn parse_odf_datetime_invalid_format_returns_none() {
    assert!(parse_odf_datetime("not a date").is_none());
}

#[test]
fn parse_odf_datetime_pre_epoch_rfc3339_returns_none() {
    assert!(parse_odf_datetime("1969-12-31T00:00:00Z").is_none());
}

#[test]
fn parse_odf_datetime_pre_epoch_naive_returns_none() {
    assert!(parse_odf_datetime("1969-12-31T00:00:00").is_none());
}

// ============= scan_element_text =============

#[test]
fn scan_element_text_with_attributes() {
    let buf = b"<meta:creation-date xmlns:meta=\"...\">2017-02-14T10:30:00Z</meta:creation-date>";
    let text = scan_element_text(buf, b"<meta:creation-date", b"</meta:creation-date>").unwrap();
    assert_eq!(text, "2017-02-14T10:30:00Z");
}

#[test]
fn scan_element_text_no_open_tag_returns_none() {
    assert!(scan_element_text(b"<other>x</other>", b"<meta:creation-date", b"</meta:creation-date>").is_none());
}

#[test]
fn scan_element_text_no_gt_returns_none() {
    assert!(scan_element_text(b"<meta:creation-date attr=\"", b"<meta:creation-date", b"</meta:creation-date>").is_none());
}

#[test]
fn scan_element_text_no_close_returns_none() {
    assert!(scan_element_text(b"<meta:creation-date>text", b"<meta:creation-date", b"</meta:creation-date>").is_none());
}

#[test]
fn scan_element_text_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"<meta:creation-date>".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</meta:creation-date>");
    assert!(scan_element_text(&buf, b"<meta:creation-date", b"</meta:creation-date>").is_none());
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
