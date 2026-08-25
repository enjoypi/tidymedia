#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::manual_is_multiple_of,
    clippy::redundant_clone,
    clippy::single_char_pattern,
    clippy::unnecessary_trailing_comma
)]

use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use tidymedia::{
    extract_office_text, mm_collect_text_attrs, mm_extract_dates, mm_extract_text, mm_parse,
    pdf_collect_string_literals, pdf_extract_text, pdf_extract_text_from_buf, pdf_parse,
    populate_office_dates, strip_markup_into, text_extract_text, truncate_at_boundary,
};

#[derive(Debug)]
struct FailReader;

impl Read for FailReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("injected read failure"))
    }
}

impl Seek for FailReader {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

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

#[test]
fn pdf_parse_extracts_head_window_dates() {
    let pdf =
        b"%PDF-1.4\n<< /CreationDate (D:20170214103000Z) /ModDate (D:20180101120000Z) >>".to_vec();
    let mut reader = Cursor::new(pdf);
    assert_eq!(
        pdf_parse(&mut reader, "application/pdf"),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn pdf_extract_text_happy_and_read_error() {
    let pdf =
        b"%PDF-1.4\n1 0 obj << /Length 24 >>\nstream\nBT (Hello from pdf) ET\nendstream\n%%EOF"
            .to_vec();
    let mut reader = Cursor::new(pdf);
    assert_eq!(
        pdf_extract_text(&mut reader, "application/pdf", 64),
        "Hello from pdf"
    );
    let mut failing = FailReader;
    assert_eq!(pdf_extract_text(&mut failing, "application/pdf", 64), "");
}

#[test]
fn pdf_extract_text_from_buf_flate_stream() {
    let mut compressed = Vec::new();
    {
        let mut enc =
            flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default());
        enc.write_all(b"BT (flate text) ET").unwrap();
        enc.finish().unwrap();
    }
    let mut pdf = b"%PDF\n<< /Filter /FlateDecode /Length 200 >>\nstream\n".to_vec();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\nendstream\n%%EOF");
    assert_eq!(pdf_extract_text_from_buf(&pdf, 64), "flate text");
}

#[test]
fn pdf_collect_string_literals_trailing_backslash() {
    let mut out = String::new();
    pdf_collect_string_literals(b"BT (abc\\", &mut out, 64);
    assert_eq!(out, "abc\\");
}

#[test]
fn pdf_collect_string_literals_escapes_then_appends_to_nonempty() {
    let mut out = String::new();
    pdf_collect_string_literals(b"BT (\\(esc\\)) ET", &mut out, 64);
    assert_eq!(out, "(esc)");
    pdf_collect_string_literals(b"BT (second) ET", &mut out, 64);
    assert_eq!(out, "(esc) second");
}

#[test]
fn strip_markup_into_folds_and_truncates() {
    let mut out = String::new();
    strip_markup_into(b"<p>hello <b>world</b></p>", &mut out, 64);
    assert_eq!(out, "hello world");
    truncate_at_boundary(&mut out, 4);
    assert_eq!(out, "hell");
}

#[test]
fn strip_markup_into_empty_result_skips_push() {
    let mut out = String::new();
    strip_markup_into(b"<a>   </a>", &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn text_extract_text_happy_and_read_error() {
    let mut reader = Cursor::new(b"hello world".to_vec());
    assert_eq!(
        text_extract_text(&mut reader, "text/plain", 64),
        "hello world"
    );
    let mut failing = FailReader;
    assert_eq!(text_extract_text(&mut failing, "text/plain", 64), "");
}

#[test]
fn extract_office_text_routes_rtf_text_mime() {
    let mut reader = Cursor::new(br"{\rtf1 body}".to_vec());
    assert_eq!(extract_office_text(&mut reader, "text/rtf", 64), "body");
}

#[test]
fn populate_office_dates_routes_rtf_text_mime() {
    let rtf = br"{\rtf1\ansi{\info{\creatim\yr2017\mo2\dy14\hr10\min30\sec0}{\revtim\yr2018\mo1\dy1\hr12\min0\sec0}}}";
    let mut reader = Cursor::new(rtf.to_vec());
    let (c, m) = populate_office_dates(&mut reader, "text/rtf");
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}
