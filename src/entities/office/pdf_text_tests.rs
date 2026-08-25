//! `pdf_text` 文本层提取单测：`extract_text_from_buf` / `collect_string_literals` /
//! `inflate_capped` / `skip_stream_eol`。原属 `pdf_tests.rs`，拆分避免超 512 行。

use super::*;

#[test]
fn text_from_uncompressed_stream_collects_tj_literals() {
    let pdf = b"%PDF-1.4\n<< /Length 40 >>\nstream\nBT (Hello) Tj (World) Tj ET\nendstream\n";
    let out = extract_text_from_buf(pdf, 256);
    assert_eq!(out, "Hello World");
}

#[test]
fn text_from_flate_stream_inflates_then_collects() {
    use std::io::Write;
    let content = b"BT (compressed body text) Tj ET";
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(content).unwrap();
    let deflated = enc.finish().unwrap();
    let mut pdf = b"<< /Filter /FlateDecode >>\nstream\n".to_vec();
    pdf.extend_from_slice(&deflated);
    pdf.extend_from_slice(b"\nendstream\n");
    let out = extract_text_from_buf(&pdf, 256);
    assert!(out.contains("compressed body text"), "got: {out}");
}

#[test]
fn text_from_flate_stream_garbage_data_skipped() {
    let pdf = b"<< /Filter /FlateDecode >>\nstream\nnot zlib at all\nendstream\n";
    assert_eq!(extract_text_from_buf(pdf, 256), "");
}

#[test]
fn text_from_buf_without_stream_keyword_empty() {
    assert_eq!(extract_text_from_buf(b"%PDF-1.4 no content", 256), "");
}

#[test]
fn text_from_buf_stream_without_endstream_empty() {
    assert_eq!(extract_text_from_buf(b"<< >>\nstream\nBT (x) Tj", 256), "");
}

#[test]
fn text_from_buf_respects_budget() {
    let pdf = b"<< >>\nstream\nBT (abcdefghijklmnop) Tj ET\nendstream\n";
    let out = extract_text_from_buf(pdf, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn literals_skipped_when_no_bt_operator() {
    let mut out = String::new();
    collect_string_literals(b"(graphics only) re f", &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn literals_unescape_parens_and_backslash() {
    let mut out = String::new();
    collect_string_literals(br"BT (a\(b\)c\\d) Tj ET", &mut out, 64);
    assert_eq!(out, r"a(b)c\d");
}

#[test]
fn literals_nested_parens_kept() {
    let mut out = String::new();
    collect_string_literals(b"BT (outer (inner) tail) Tj ET", &mut out, 64);
    assert_eq!(out, "outer (inner) tail");
}

#[test]
fn literals_empty_string_no_trailing_space() {
    let mut out = String::new();
    collect_string_literals(b"BT () Tj ET", &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn literals_unterminated_paren_lenient() {
    let mut out = String::new();
    collect_string_literals(b"BT (never closed", &mut out, 64);
    assert_eq!(out, "never closed");
}

#[test]
fn literals_other_escape_sequences_dropped() {
    let mut out = String::new();
    collect_string_literals(br"BT (a\nb) Tj ET", &mut out, 64);
    assert_eq!(out, "ab");
}

#[test]
fn literals_trailing_backslash_treated_as_literal() {
    let mut out = String::new();
    collect_string_literals(b"BT (abc\\", &mut out, 64);
    assert_eq!(out, "abc\\");
}

#[test]
fn literals_appends_to_nonempty_out() {
    let mut out = String::from("existing");
    collect_string_literals(b"BT (new) Tj ET", &mut out, 64);
    assert_eq!(out, "existing new");
}

#[test]
fn inflate_capped_valid_zlib_roundtrip() {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(b"payload").unwrap();
    let z = enc.finish().unwrap();
    assert_eq!(inflate_capped(&z).unwrap(), b"payload");
}

#[test]
fn inflate_capped_garbage_returns_none() {
    assert!(inflate_capped(b"definitely not zlib").is_none());
}

#[test]
fn inflate_capped_empty_output_returns_none() {
    let enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let z = enc.finish().unwrap();
    assert!(inflate_capped(&z).is_none());
}

#[test]
fn skip_stream_eol_variants() {
    assert_eq!(skip_stream_eol(b"s\r\nX", 1), 3);
    assert_eq!(skip_stream_eol(b"s\nX", 1), 2);
    assert_eq!(skip_stream_eol(b"sX", 1), 1);
}
