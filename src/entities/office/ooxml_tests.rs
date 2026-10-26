use std::io::Cursor;

use crate::entities::office::tests_common::{corrupt_zip_entry, make_zip};

use super::*;

#[test]
fn parse_happy_path_extracts_dates() {
    let z = make_zip(&[(
        "docProps/core.xml",
        br"<dcterms:created>2017-02-14T10:30:00Z</dcterms:created><dcterms:modified>2018-01-01T12:00:00Z</dcterms:modified>".as_slice(),
    )]);
    let mut r = Cursor::new(z);
    assert_eq!(
        parse(
            &mut r,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn parse_missing_core_xml_returns_zeros() {
    let z = make_zip(&[("word/document.xml", b"<w:document/>".as_slice())]);
    let mut r = Cursor::new(z);
    assert_eq!(
        parse(
            &mut r,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ),
        (0, 0)
    );
}

#[test]
fn parse_corrupt_core_xml_read_error_returns_zeros() {
    let mut z = make_zip(&[(
        "docProps/core.xml",
        b"<dcterms:created>2017-02-14T10:30:00Z</dcterms:created>".as_slice(),
    )]);
    corrupt_zip_entry(&mut z, b"2017-02-14");
    let mut r = Cursor::new(z);
    assert_eq!(
        parse(
            &mut r,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ),
        (0, 0)
    );
}

#[test]
fn extract_text_docx_missing_document_xml_returns_empty() {
    let z = make_zip(&[("docProps/core.xml", b"<x/>".as_slice())]);
    let mut r = Cursor::new(z);
    assert_eq!(
        extract_text(
            &mut r,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            64
        ),
        ""
    );
}

#[test]
fn extract_text_xlsx_missing_shared_strings_returns_empty() {
    let z = make_zip(&[("xl/workbook.xml", b"<x/>".as_slice())]);
    let mut r = Cursor::new(z);
    assert_eq!(
        extract_text(
            &mut r,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            64
        ),
        ""
    );
}

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

#[test]
fn scan_element_text_with_attributes() {
    let buf =
        br#"<dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>"#;
    let text = scan_element_text(buf, b"<dcterms:created", b"</dcterms:created>").unwrap();
    assert_eq!(text, "2017-02-14T10:30:00Z");
}

#[test]
fn scan_element_text_no_open_tag_returns_none() {
    assert!(
        scan_element_text(
            b"<other>x</other>",
            b"<dcterms:created",
            b"</dcterms:created>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_no_gt_after_open_returns_none() {
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

#[test]
fn parse_iso8601_z_utc() {
    assert_eq!(
        parse_iso8601_to_epoch("2017-02-14T10:30:00Z"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_with_offset() {
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
