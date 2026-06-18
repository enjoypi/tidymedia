//! EPUB 单测：覆盖 `find_opf_path` + `extract_dates` + `scan_meta_property` 等 helper。

use super::*;

// ============= extract_dates =============

#[test]
fn extract_dates_happy_path() {
    let opf = br#"<package>
  <metadata>
    <dc:date>2017-02-14T10:30:00Z</dc:date>
    <meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>
  </metadata>
</package>"#;
    let (c, m) = extract_dates(opf);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_missing_modified_returns_zero() {
    let opf = b"<dc:date>2017-02-14T10:30:00Z</dc:date>";
    let (c, m) = extract_dates(opf);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_missing_dc_date_returns_zero_for_created() {
    let opf = br#"<meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>"#;
    let (c, m) = extract_dates(opf);
    assert_eq!(c, 0);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_invalid_dates_returns_zeros() {
    let opf = br#"<dc:date>not a date</dc:date><meta property="dcterms:modified">also not</meta>"#;
    assert_eq!(extract_dates(opf), (0, 0));
}

// ============= find_opf_path =============

#[test]
fn find_opf_path_basic() {
    let xml = br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="..."/></rootfiles></container>"#;
    assert_eq!(find_opf_path(xml).as_deref(), Some("OEBPS/content.opf"));
}

#[test]
fn find_opf_path_missing_attr_returns_none() {
    assert!(find_opf_path(b"<container/>").is_none());
}

#[test]
fn find_opf_path_no_closing_quote_returns_none() {
    let xml = br#"<rootfile full-path="OEBPS/content.opf"#;
    assert!(find_opf_path(xml).is_none());
}

#[test]
fn find_opf_path_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"<rootfile full-path=\"".to_vec();
    buf.push(0xff);
    buf.push(b'"');
    assert!(find_opf_path(&buf).is_none());
}

// ============= scan_meta_property =============

#[test]
fn scan_meta_property_basic() {
    let buf = br#"<meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>"#;
    let text = scan_meta_property(buf, b"dcterms:modified").unwrap();
    assert_eq!(text, "2018-01-01T12:00:00Z");
}

#[test]
fn scan_meta_property_with_id_before_property() {
    let buf = br#"<meta id="m1" property="dcterms:modified">2018-01-01T12:00:00Z</meta>"#;
    let text = scan_meta_property(buf, b"dcterms:modified").unwrap();
    assert_eq!(text, "2018-01-01T12:00:00Z");
}

#[test]
fn scan_meta_property_no_match_returns_none() {
    assert!(scan_meta_property(b"<other/>", b"dcterms:modified").is_none());
}

#[test]
fn scan_meta_property_no_gt_returns_none() {
    let buf = br#"<meta property="dcterms:modified""#;
    assert!(scan_meta_property(buf, b"dcterms:modified").is_none());
}

#[test]
fn scan_meta_property_no_close_meta_returns_none() {
    let buf = br#"<meta property="dcterms:modified">text without close"#;
    assert!(scan_meta_property(buf, b"dcterms:modified").is_none());
}

#[test]
fn scan_meta_property_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"<meta property=\"dcterms:modified\">".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</meta>");
    assert!(scan_meta_property(&buf, b"dcterms:modified").is_none());
}

// ============= scan_element_text =============

#[test]
fn scan_element_text_basic() {
    let buf = b"<dc:date>2017-02-14T10:30:00Z</dc:date>";
    let text = scan_element_text(buf, b"<dc:date", b"</dc:date>").unwrap();
    assert_eq!(text, "2017-02-14T10:30:00Z");
}

#[test]
fn scan_element_text_no_open_returns_none() {
    assert!(scan_element_text(b"<other/>", b"<dc:date", b"</dc:date>").is_none());
}

#[test]
fn scan_element_text_no_gt_returns_none() {
    let buf = b"<dc:date xml:lang=\"";
    assert!(scan_element_text(buf, b"<dc:date", b"</dc:date>").is_none());
}

#[test]
fn scan_element_text_no_close_returns_none() {
    let buf = b"<dc:date>text without close";
    assert!(scan_element_text(buf, b"<dc:date", b"</dc:date>").is_none());
}

#[test]
fn scan_element_text_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"<dc:date>".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</dc:date>");
    assert!(scan_element_text(&buf, b"<dc:date", b"</dc:date>").is_none());
}

// ============= parse_iso8601_to_epoch =============

#[test]
fn parse_iso8601_basic() {
    assert_eq!(parse_iso8601_to_epoch("2017-02-14T10:30:00Z"), Some(1_487_068_200));
}

#[test]
fn parse_iso8601_invalid_returns_none() {
    assert!(parse_iso8601_to_epoch("not a date").is_none());
}

#[test]
fn parse_iso8601_pre_epoch_returns_none() {
    assert!(parse_iso8601_to_epoch("1969-12-31T00:00:00Z").is_none());
}

// ============= find_byte / find_subslice =============

#[test]
fn find_byte_found_and_not_found() {
    assert_eq!(find_byte(b"a\"b", b'"'), Some(1));
    assert!(find_byte(b"abc", b'"').is_none());
}

#[test]
fn find_subslice_found_and_not_found() {
    assert_eq!(find_subslice(b"hello world", b"world"), Some(6));
    assert!(find_subslice(b"hello", b"world").is_none());
}
