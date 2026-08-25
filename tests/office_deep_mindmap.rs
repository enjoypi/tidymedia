use std::io::{Cursor, Read, Seek, Write};

use tidymedia::mindmap_zip::{
    collect_json_titles, extract_dates_from_json, extract_text, find_subslice, millis_to_secs,
    parse, parse_xmind, read_entry_capped,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MIME_XMIND: &str = "application/vnd.xmind.workbook";
const MIME_XMIND_ALT: &str = "application/x-xmind";
const MIME_MINDNODE: &str = "application/x-mindnode";
const METADATA_JSON: &str = "metadata.json";
const CONTENT_XML: &str = "content.xml";
const CONTENT_JSON: &str = "content.json";

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(
            *name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .expect("start zip entry");
        w.write_all(data).expect("write zip entry");
    }
    w.finish().expect("finish zip").into_inner()
}
fn data_region(zip: &[u8], data_len: usize) -> (u64, u64) {
    let central = zip
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .expect("central directory signature");
    let hi = central as u64;
    (hi - data_len as u64, hi)
}
#[derive(Debug)]
struct FailWindow {
    inner: Cursor<Vec<u8>>,
    lo: u64,
    hi: u64,
}

impl Read for FailWindow {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let pos = self.inner.position();
        if (self.lo..self.hi).contains(&pos) {
            return Err(std::io::Error::other("injected read failure"));
        }
        self.inner.read(buf)
    }
}

impl Seek for FailWindow {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

fn valid_xmind_zip() -> Vec<u8> {
    zip_bytes(&[(
        METADATA_JSON,
        br#"{"created": 1487068200000, "modified": 1514808000000}"#,
    )])
}

#[test]
fn parse_invalid_zip_returns_zeros() {
    let mut reader = Cursor::new(b"not a zip".to_vec());
    assert_eq!(parse(&mut reader, MIME_XMIND), (0, 0));
}

#[test]
fn parse_xmind_happy_extracts_dates() {
    let mut reader = Cursor::new(valid_xmind_zip());
    assert_eq!(
        parse(&mut reader, MIME_XMIND),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn parse_xmind_alt_mime_extracts_dates() {
    let mut reader = Cursor::new(valid_xmind_zip());
    assert_eq!(
        parse(&mut reader, MIME_XMIND_ALT),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn parse_non_xmind_mime_returns_zeros() {
    let mut reader = Cursor::new(valid_xmind_zip());
    assert_eq!(parse(&mut reader, MIME_MINDNODE), (0, 0));
}

#[test]
fn parse_xmind_missing_metadata_json_returns_zeros() {
    let zip = zip_bytes(&[(CONTENT_XML, b"<topic/>")]);
    let mut archive = ZipArchive::new(Cursor::new(zip)).expect("open zip");
    assert_eq!(parse_xmind(&mut archive), (0, 0));
}

#[test]
fn parse_xmind_read_error_returns_zeros() {
    let content: &[u8] = br#"{"created": 1487068200000}"#;
    let zip = zip_bytes(&[(METADATA_JSON, content)]);
    let (lo, hi) = data_region(&zip, content.len());
    let mut archive = ZipArchive::new(FailWindow {
        inner: Cursor::new(zip),
        lo,
        hi,
    })
    .expect("open zip");
    assert_eq!(parse_xmind(&mut archive), (0, 0));
}

#[test]
fn parse_xmind_missing_entry_in_failing_reader_returns_zeros() {
    let content: &[u8] = b"<topic/>";
    let zip = zip_bytes(&[(CONTENT_XML, content)]);
    let (lo, hi) = data_region(&zip, content.len());
    let mut archive = ZipArchive::new(FailWindow {
        inner: Cursor::new(zip),
        lo,
        hi,
    })
    .expect("open zip");
    assert_eq!(parse_xmind(&mut archive), (0, 0));
}

#[test]
fn read_entry_capped_missing_returns_none() {
    let zip = zip_bytes(&[(CONTENT_JSON, b"{}")]);
    let mut archive = ZipArchive::new(Cursor::new(zip)).expect("open zip");
    assert!(read_entry_capped(&mut archive, CONTENT_XML).is_none());
}

#[test]
fn read_entry_capped_present_returns_some() {
    let zip = zip_bytes(&[(CONTENT_XML, b"<a>text</a>")]);
    let mut archive = ZipArchive::new(Cursor::new(zip)).expect("open zip");
    assert_eq!(
        read_entry_capped(&mut archive, CONTENT_XML),
        Some(b"<a>text</a>".to_vec())
    );
}

#[test]
fn read_entry_capped_read_error_returns_none() {
    let content: &[u8] = b"<a>text</a>";
    let zip = zip_bytes(&[(CONTENT_XML, content)]);
    let (lo, hi) = data_region(&zip, content.len());
    let mut archive = ZipArchive::new(FailWindow {
        inner: Cursor::new(zip),
        lo,
        hi,
    })
    .expect("open zip");
    assert!(read_entry_capped(&mut archive, CONTENT_XML).is_none());
}

#[test]
fn extract_dates_happy_path() {
    let json = br#"{"created": 1487068200000, "modified": 1514808000000}"#;
    assert_eq!(
        extract_dates_from_json(json),
        (1_487_068_200, 1_514_808_000)
    );
}

#[test]
fn extract_dates_missing_field_returns_zero() {
    assert_eq!(
        extract_dates_from_json(br#"{"created": 1487068200000}"#),
        (1_487_068_200, 0)
    );
    assert_eq!(
        extract_dates_from_json(br#"{"modified": 1514808000000}"#),
        (0, 1_514_808_000)
    );
    assert_eq!(extract_dates_from_json(b"{}"), (0, 0));
}

#[test]
fn extract_dates_invalid_json_and_extra_fields() {
    assert_eq!(extract_dates_from_json(b"not json"), (0, 0));
    assert_eq!(
        extract_dates_from_json(br#"{"creator":"x","created":1487068200000,"other":42}"#),
        (1_487_068_200, 0)
    );
}

#[test]
fn millis_to_secs_boundaries() {
    assert_eq!(millis_to_secs(1_487_068_200_000), Some(1_487_068_200));
    assert!(millis_to_secs(60_000).is_none());
    assert!(millis_to_secs(86_399_999).is_none());
    assert_eq!(millis_to_secs(86_400_000), Some(86_400));
}

#[test]
fn json_titles_collects_nested_and_escaped() {
    let mut out = String::new();
    collect_json_titles(
        r#"[{"title":"发票主题","children":{"attached":[{"title":"子题"}]}}]"#.as_bytes(),
        &mut out,
        64,
    );
    assert_eq!(out, "发票主题 子题");

    let mut escaped = String::new();
    collect_json_titles(br#"{"title":"say \"hi\" here"}"#, &mut escaped, 64);
    assert_eq!(escaped, "say \\\"hi\\\" here");
}

#[test]
fn json_titles_skips_non_string_and_empty() {
    let mut out = String::new();
    collect_json_titles(br#"{"title": 123, "title":"real"}"#, &mut out, 64);
    assert_eq!(out, "real");

    let mut empty = String::new();
    collect_json_titles(br#"{"title":"","title":"x"}"#, &mut empty, 64);
    assert_eq!(empty, "x");
}

#[test]
fn json_titles_whitespace_after_colon() {
    let mut out = String::new();
    collect_json_titles(b"{\"title\":   \"spaced\"}", &mut out, 64);
    assert_eq!(out, "spaced");
}

#[test]
fn json_titles_budget_truncates() {
    let mut out = String::new();
    collect_json_titles(br#"{"title":"abcdefgh"}"#, &mut out, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn json_titles_key_at_buffer_end_short_circuits_whitespace_scan() {
    let mut out = String::new();
    collect_json_titles(b"{\"title\":", &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn json_titles_unterminated_value_scans_to_end() {
    let mut out = String::new();
    collect_json_titles(b"{\"title\":\"abc", &mut out, 64);
    assert_eq!(out, "abc");
}

#[test]
fn find_subslice_locates_first_occurrence() {
    assert_eq!(find_subslice(b"abcXdef", b"X"), Some(3));
    assert_eq!(find_subslice(b"abc", b"zzz"), None);
    assert_eq!(find_subslice(b"", b"x"), None);
}

#[test]
fn extract_text_invalid_zip_returns_empty() {
    let mut reader = Cursor::new(b"not a zip".to_vec());
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "");
}

#[test]
fn extract_text_content_xml_wins_over_json() {
    let zip = zip_bytes(&[
        (CONTENT_XML, b"<a>from xml</a>"),
        (CONTENT_JSON, br#"{"title":"from json"}"#),
    ]);
    let mut reader = Cursor::new(zip);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "from xml");
}

#[test]
fn extract_text_content_json_fallback() {
    let zip = zip_bytes(&[(CONTENT_JSON, br#"{"title":"from json"}"#)]);
    let mut reader = Cursor::new(zip);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "from json");
}

#[test]
fn extract_text_no_content_entries_yields_empty() {
    let zip = zip_bytes(&[("other.txt", b"x")]);
    let mut reader = Cursor::new(zip);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "");
}
