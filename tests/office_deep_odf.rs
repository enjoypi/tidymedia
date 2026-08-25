//! ODF 容器/文本提取 deep 测试：`odf_parse` / `odf_extract_text` 的 zip 容器防御
//! 分支（zip 打开失败 / entry 缺失 / 读 CRC 失败）+ `parse_odf_datetime` /
//! `scan_element_text` / `extract_dates` 纯函数分支。unit helper 不可见，输入从
//! `odf_tests.rs` 复制。
#![allow(clippy::single_char_pattern)]

use std::io::{Cursor, Write};

use tidymedia::{
    odf_extract_dates, odf_extract_text, odf_parse, odf_parse_odf_datetime, odf_scan_element_text,
};

const MIME_ODT: &str = "application/vnd.oasis.opendocument.text";

const META_XML: &[u8] = b"<office:meta><meta:creation-date>2017-02-14T10:30:00Z</meta:creation-date><dc:date>2018-01-01T12:00:00Z</dc:date></office:meta>";

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

fn parse_bytes(bytes: &[u8]) -> (u64, u64) {
    odf_parse(&mut Cursor::new(bytes.to_vec()), MIME_ODT)
}

fn extract_bytes(bytes: &[u8], max_bytes: usize) -> String {
    odf_extract_text(&mut Cursor::new(bytes.to_vec()), MIME_ODT, max_bytes)
}

// ============= odf_parse 容器分支 =============

#[test]
fn parse_happy_path_reads_both_dates() {
    let zip = make_zip(&[("meta.xml", META_XML)]);
    assert_eq!(parse_bytes(&zip), (1_487_068_200, 1_514_808_000));
}

#[test]
fn parse_garbage_bytes_returns_zero() {
    assert_eq!(parse_bytes(b"not a zip at all"), (0, 0));
}

#[test]
fn parse_zip_missing_meta_xml_returns_zero() {
    let zip = make_zip(&[("content.xml", b"<office:body/>")]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_corrupt_meta_xml_data_returns_zero() {
    let mut zip = make_zip(&[("meta.xml", META_XML)]);
    flip_byte(&mut zip, b"meta:creation-date");
    assert_eq!(parse_bytes(&zip), (0, 0));
}

// ============= odf_extract_text 容器分支 =============

#[test]
fn extract_text_garbage_bytes_returns_empty() {
    assert_eq!(extract_bytes(b"not a zip at all", 64), "");
}

#[test]
fn extract_text_happy_path_extracts_content_xml() {
    let zip = make_zip(&[(
        "content.xml",
        "<office:body><text:p>合同条款正文</text:p></office:body>".as_bytes(),
    )]);
    assert_eq!(extract_bytes(&zip, 64), "合同条款正文");
}

#[test]
fn extract_text_zip_missing_content_xml_returns_empty() {
    let zip = make_zip(&[("meta.xml", META_XML)]);
    assert_eq!(extract_bytes(&zip, 64), "");
}

#[test]
fn extract_text_corrupt_content_xml_data_returns_empty() {
    let mut zip = make_zip(&[(
        "content.xml",
        b"<office:body><text:p>hi</text:p></office:body>",
    )]);
    flip_byte(&mut zip, b"office:body");
    assert_eq!(extract_bytes(&zip, 64), "");
}

// ============= extract_dates 纯函数 =============

#[test]
fn extract_dates_naive_datetime_treats_as_utc() {
    let xml = b"<meta:creation-date>2017-02-14T10:30:00</meta:creation-date>";
    assert_eq!(odf_extract_dates(xml), (1_487_068_200, 0));
}

#[test]
fn extract_dates_missing_both_returns_zeros() {
    assert_eq!(odf_extract_dates(b"<no relevant tags />"), (0, 0));
}

// ============= parse_odf_datetime 边界 =============

#[test]
fn parse_odf_datetime_rfc3339_with_z() {
    assert_eq!(
        odf_parse_odf_datetime("2017-02-14T10:30:00Z"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_odf_datetime_rfc3339_with_offset() {
    assert_eq!(
        odf_parse_odf_datetime("2017-02-14T18:30:00+08:00"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_odf_datetime_naive_falls_back_to_utc() {
    assert_eq!(
        odf_parse_odf_datetime("2017-02-14T10:30:00"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_odf_datetime_invalid_format_returns_none() {
    assert!(odf_parse_odf_datetime("not a date").is_none());
}

#[test]
fn parse_odf_datetime_pre_epoch_rfc3339_returns_none() {
    assert!(odf_parse_odf_datetime("1969-12-31T00:00:00Z").is_none());
}

#[test]
fn parse_odf_datetime_pre_epoch_naive_returns_none() {
    assert!(odf_parse_odf_datetime("1969-12-31T00:00:00").is_none());
}

// ============= scan_element_text 边界 =============

#[test]
fn scan_element_text_with_attributes() {
    let buf = b"<meta:creation-date xmlns:meta=\"...\">2017-02-14T10:30:00Z</meta:creation-date>";
    assert_eq!(
        odf_scan_element_text(buf, b"<meta:creation-date", b"</meta:creation-date>"),
        Some("2017-02-14T10:30:00Z")
    );
}

#[test]
fn scan_element_text_no_open_tag_returns_none() {
    assert!(
        odf_scan_element_text(
            b"<other>x</other>",
            b"<meta:creation-date",
            b"</meta:creation-date>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_no_gt_returns_none() {
    assert!(
        odf_scan_element_text(
            b"<meta:creation-date attr=\"",
            b"<meta:creation-date",
            b"</meta:creation-date>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_no_close_returns_none() {
    assert!(
        odf_scan_element_text(
            b"<meta:creation-date>text",
            b"<meta:creation-date",
            b"</meta:creation-date>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_non_utf8_returns_none() {
    let mut buf: Vec<u8> = b"<meta:creation-date>".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</meta:creation-date>");
    assert!(
        odf_scan_element_text(&buf, b"<meta:creation-date", b"</meta:creation-date>").is_none()
    );
}
