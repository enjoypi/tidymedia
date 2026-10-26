use std::io::{Cursor, Write};

use super::{
    collect_string_literals as pdf_collect_string_literals, extract_text as pdf_extract_text,
    extract_text_from_buf as pdf_extract_text_from_buf, parse as pdf_parse,
};
use crate::entities::office::tests_common::FailReader;

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
