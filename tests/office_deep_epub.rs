#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::manual_is_multiple_of,
    clippy::redundant_clone,
    clippy::single_char_pattern,
    clippy::unnecessary_trailing_comma
)]

use std::io::{Cursor, Write};

use tidymedia::{epub_extract_text, epub_parse};

const CONTAINER: &str = "META-INF/container.xml";
const OPF_PATH: &str = "OEBPS/content.opf";
const CONTAINER_XML: &[u8] = br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
const OPF_XML: &[u8] = b"<package><metadata><dc:date>2017-02-14T10:30:00Z</dc:date><meta property=\"dcterms:modified\">2018-01-01T12:00:00Z</meta></metadata></package>";

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

fn corrupt_nth_local_header(buf: &mut [u8], n: usize) {
    let mut seen = 0;
    for i in 0..buf.len() - 4 {
        if &buf[i..i + 4] == b"PK\x03\x04" {
            seen += 1;
            if seen == n {
                buf[i] ^= 1;
                return;
            }
        }
    }
    panic!("only {seen} local headers, wanted {n}");
}

fn parse_bytes(bytes: &[u8]) -> (u64, u64) {
    epub_parse(&mut Cursor::new(bytes.to_vec()), "application/epub+zip")
}

fn extract_bytes(bytes: &[u8], max_bytes: usize) -> String {
    epub_extract_text(
        &mut Cursor::new(bytes.to_vec()),
        "application/epub+zip",
        max_bytes,
    )
}

#[test]
fn parse_happy_path_reads_both_dates() {
    let zip = make_zip(&[(CONTAINER, CONTAINER_XML), (OPF_PATH, OPF_XML)]);
    assert_eq!(parse_bytes(&zip), (1_487_068_200, 1_514_808_000));
}

#[test]
fn parse_garbage_bytes_returns_zero() {
    assert_eq!(parse_bytes(b"not a zip at all"), (0, 0));
}

#[test]
fn parse_missing_container_xml_returns_zero() {
    let zip = make_zip(&[("README.txt", b"hi")]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_container_without_rootfile_returns_zero() {
    let zip = make_zip(&[(CONTAINER, br"<container/>")]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_opf_entry_missing_returns_zero() {
    let zip = make_zip(&[(CONTAINER, CONTAINER_XML)]);
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn parse_corrupt_container_data_returns_zero() {
    let mut zip = make_zip(&[(CONTAINER, CONTAINER_XML), (OPF_PATH, OPF_XML)]);
    flip_byte(&mut zip, b"rootfiles");
    assert_eq!(parse_bytes(&zip), (0, 0));
}

#[test]
fn extract_text_garbage_bytes_returns_empty() {
    assert_eq!(extract_bytes(b"not a zip at all", 64), "");
}

#[test]
fn extract_text_keeps_xhtml_html_htm() {
    let zip = make_zip(&[
        ("a.xhtml", b"<p>alpha</p>"),
        ("b.HTML", b"<p>bravo</p>"),
        ("c.htm", b"<p>charlie</p>"),
    ]);
    assert_eq!(extract_bytes(&zip, 64), "alpha bravo charlie");
}

#[test]
fn extract_text_budget_breaks_before_second_entry() {
    let zip = make_zip(&[("a.xhtml", b"<p>ab</p>"), ("b.xhtml", b"<p>cd</p>")]);
    assert_eq!(extract_bytes(&zip, 1), "a");
}

#[test]
fn extract_text_skips_entry_with_corrupt_local_header() {
    let mut zip = make_zip(&[("a.xhtml", b"alpha"), ("b.xhtml", b"bravo")]);
    corrupt_nth_local_header(&mut zip, 2);
    assert_eq!(extract_bytes(&zip, 64), "alpha");
}

#[test]
fn extract_text_skips_entry_with_corrupt_data() {
    let mut zip = make_zip(&[("a.xhtml", b"alpha"), ("b.xhtml", b"bravo")]);
    flip_byte(&mut zip, b"bravo");
    assert_eq!(extract_bytes(&zip, 64), "alpha");
}
