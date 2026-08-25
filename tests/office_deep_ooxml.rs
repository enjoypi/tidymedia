//! OOXML 容器/文本提取 deep 测试：`ooxml_parse` / `ooxml_extract_text` 的 zip 容器
//! 防御分支（zip 打开失败 / entry 缺失 / 读 CRC 失败 / PPTX slide 过滤与预算断点）
//! 在 release 实例直接构造 bytes 覆盖；纯函数分支从 `ooxml_tests.rs` 复制（helper
//! 不可见），防 multi-instance phantom miss。
#![allow(
    clippy::cast_possible_truncation,
    clippy::single_char_pattern,
    clippy::unnecessary_wraps
)]

use std::io::{Cursor, Write};

use tidymedia::{
    ooxml_extract_dates, ooxml_extract_text, ooxml_parse, ooxml_parse_iso8601_to_epoch,
    ooxml_scan_element_text,
};

const MIME_DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const MIME_PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const MIME_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

const CORE_XML: &[u8] = br#"<cp:coreProperties xmlns:cp="..." xmlns:dcterms="http://purl.org/dc/terms/"><dcterms:created>2017-02-14T10:30:00Z</dcterms:created><dcterms:modified>2018-01-01T12:00:00Z</dcterms:modified></cp:coreProperties>"#;

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
    ooxml_parse(&mut Cursor::new(bytes.to_vec()), MIME_DOCX)
}

fn extract_bytes(bytes: &[u8], mime: &str, max_bytes: usize) -> String {
    ooxml_extract_text(&mut Cursor::new(bytes.to_vec()), mime, max_bytes)
}

// ============= ooxml_parse 容器分支 =============

#[test]
fn parse_happy_path_reads_both_dates() {
    let zip = make_zip(&[("docProps/core.xml", CORE_XML)]);
    assert_eq!(parse_bytes(&zip), (1_487_068_200, 1_514_808_000));
}

#[test]
fn parse_garbage_bytes_returns_zero() {
    assert_eq!(parse_bytes(b"not a zip at all"), (0, 0));
}

#[test]
fn parse_zip_missing_core_xml_returns_zero() {
    let zip = make_zip(&[("README.txt", b"hi")]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_corrupt_core_xml_data_returns_zero() {
    let mut zip = make_zip(&[("docProps/core.xml", CORE_XML)]);
    flip_byte(&mut zip, b"dcterms:created");
    assert_eq!(parse_bytes(&zip), (0, 0));
}

// ============= ooxml_extract_text 容器分支 =============

#[test]
fn extract_text_garbage_bytes_returns_empty() {
    assert_eq!(extract_bytes(b"not a zip at all", MIME_DOCX, 64), "");
}

#[test]
fn extract_text_docx_extracts_document_xml() {
    let zip = make_zip(&[(
        "word/document.xml",
        "<w:document><w:body><w:p><w:r><w:t>发票 报销</w:t></w:r></w:p></w:body></w:document>"
            .as_bytes(),
    )]);
    assert_eq!(extract_bytes(&zip, MIME_DOCX, 64), "发票 报销");
}

#[test]
fn extract_text_xlsx_shared_strings_missing_returns_empty() {
    let zip = make_zip(&[("xl/workbook.xml", b"<workbook/>")]);
    assert_eq!(extract_bytes(&zip, MIME_XLSX, 64), "");
}

#[test]
fn extract_text_xlsx_corrupt_shared_strings_data_returns_empty() {
    let mut zip = make_zip(&[(
        "xl/sharedStrings.xml",
        b"<sst><si><t>cell one</t></si></sst>",
    )]);
    flip_byte(&mut zip, b"<sst>");
    assert_eq!(extract_bytes(&zip, MIME_XLSX, 64), "");
}

#[test]
fn extract_text_pptx_filters_slides_by_extension() {
    let zip = make_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("ppt/slides/slide1.xml", b"<p:sp><a:t>first</a:t></p:sp>"),
        ("ppt/slides/slide2.txt", b"ignored"),
        ("ppt/slides/slide3", b"ignored"),
    ]);
    assert_eq!(extract_bytes(&zip, MIME_PPTX, 64), "first");
}

#[test]
fn extract_text_pptx_budget_breaks_between_slides() {
    let zip = make_zip(&[
        ("ppt/slides/slide1.xml", b"<p:sp><a:t>ab</a:t></p:sp>"),
        ("ppt/slides/slide2.xml", b"<p:sp><a:t>cd</a:t></p:sp>"),
    ]);
    assert_eq!(extract_bytes(&zip, MIME_PPTX, 1), "a");
}

// ============= extract_dates 纯函数 =============

#[test]
fn extract_dates_missing_modified_returns_zero() {
    let xml = b"<dcterms:created>2017-02-14T10:30:00Z</dcterms:created>";
    assert_eq!(ooxml_extract_dates(xml), (1_487_068_200, 0));
}

#[test]
fn extract_dates_no_tags_returns_zeros() {
    assert_eq!(ooxml_extract_dates(b"<no relevant tags />"), (0, 0));
}

// ============= scan_element_text 边界 =============

#[test]
fn scan_element_text_with_attributes() {
    let buf =
        br#"<dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>"#;
    assert_eq!(
        ooxml_scan_element_text(buf, b"<dcterms:created", b"</dcterms:created>"),
        Some("2017-02-14T10:30:00Z")
    );
}

#[test]
fn scan_element_text_no_open_tag_returns_none() {
    assert!(
        ooxml_scan_element_text(
            b"<other>x</other>",
            b"<dcterms:created",
            b"</dcterms:created>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_no_gt_after_open_returns_none() {
    assert!(
        ooxml_scan_element_text(
            b"<dcterms:created xsi:type=\"",
            b"<dcterms:created",
            b"</dcterms:created>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_no_close_tag_returns_none() {
    assert!(
        ooxml_scan_element_text(
            b"<dcterms:created>text",
            b"<dcterms:created",
            b"</dcterms:created>"
        )
        .is_none()
    );
}

#[test]
fn scan_element_text_non_utf8_text_returns_none() {
    let mut buf: Vec<u8> = b"<dcterms:created>".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b"</dcterms:created>");
    assert!(ooxml_scan_element_text(&buf, b"<dcterms:created", b"</dcterms:created>").is_none());
}

// ============= parse_iso8601_to_epoch 边界 =============

#[test]
fn parse_iso8601_z_utc_and_offset() {
    assert_eq!(
        ooxml_parse_iso8601_to_epoch("2017-02-14T10:30:00Z"),
        Some(1_487_068_200)
    );
    assert_eq!(
        ooxml_parse_iso8601_to_epoch("2017-02-14T18:30:00+08:00"),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_leading_whitespace_trims() {
    assert_eq!(
        ooxml_parse_iso8601_to_epoch("  2017-02-14T10:30:00Z  "),
        Some(1_487_068_200)
    );
}

#[test]
fn parse_iso8601_invalid_format_returns_none() {
    assert!(ooxml_parse_iso8601_to_epoch("not a date").is_none());
}

#[test]
fn parse_iso8601_pre_epoch_returns_none() {
    assert!(ooxml_parse_iso8601_to_epoch("1969-12-31T00:00:00Z").is_none());
}
