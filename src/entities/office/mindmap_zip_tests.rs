//! `mindmap_zip` 单测：覆盖 `extract_dates_from_json` + `millis_to_secs`。

use super::*;

#[test]
fn extract_dates_happy_path() {
    let json = br#"{"created": 1487068200000, "modified": 1514808000000}"#;
    let (c, m) = extract_dates_from_json(json);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_missing_modified_returns_zero() {
    let json = br#"{"created": 1487068200000}"#;
    let (c, m) = extract_dates_from_json(json);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_missing_created_returns_zero() {
    let json = br#"{"modified": 1514808000000}"#;
    let (c, m) = extract_dates_from_json(json);
    assert_eq!(c, 0);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_empty_object_returns_zeros() {
    assert_eq!(extract_dates_from_json(b"{}"), (0, 0));
}

#[test]
fn extract_dates_invalid_json_returns_zeros() {
    assert_eq!(extract_dates_from_json(b"not json"), (0, 0));
}

#[test]
fn extract_dates_extra_fields_ignored() {
    let json = br#"{"creator":"x","created":1487068200000,"other":42}"#;
    let (c, _) = extract_dates_from_json(json);
    assert_eq!(c, 1_487_068_200);
}

#[test]
fn millis_to_secs_modern() {
    assert_eq!(millis_to_secs(1_487_068_200_000), Some(1_487_068_200));
}

#[test]
fn millis_to_secs_pre_first_day_returns_none() {
    assert!(millis_to_secs(60_000).is_none());
}

// ============= collect_json_titles（extract_text 业务） =============

#[test]
fn json_titles_collects_nested_topics() {
    let mut out = String::new();
    collect_json_titles(
        r#"[{"title":"发票主题","children":{"attached":[{"title":"子题"}]}}]"#.as_bytes(),
        &mut out,
        64,
    );
    assert_eq!(out, "发票主题 子题");
}

#[test]
fn json_titles_skips_non_string_value() {
    let mut out = String::new();
    collect_json_titles(br#"{"title": 123, "title":"real"}"#, &mut out, 64);
    assert_eq!(out, "real");
}

#[test]
fn json_titles_keeps_escaped_quote_inside() {
    let mut out = String::new();
    collect_json_titles(br#"{"title":"a\"b"}"#, &mut out, 64);
    assert_eq!(out, "a\\\"b");
}

#[test]
fn json_titles_budget_truncates() {
    let mut out = String::new();
    collect_json_titles(br#"{"title":"abcdefgh"}"#, &mut out, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn json_titles_whitespace_after_colon() {
    let mut out = String::new();
    collect_json_titles(b"{\"title\":   \"spaced\"}", &mut out, 64);
    assert_eq!(out, "spaced");
}

#[test]
fn json_titles_empty_value_skipped() {
    let mut out = String::new();
    collect_json_titles(br#"{"title":"","title":"x"}"#, &mut out, 64);
    assert_eq!(out, "x");
}

use super::{collect_json_titles, find_subslice, millis_to_secs};

#[test]
fn collect_json_titles_extracts_title_values() {
    let json = br#"{"title":"Planning","children":[{"title":"Q1 Goals"}]}"#;
    let mut out = String::new();
    collect_json_titles(json, &mut out, 100);
    assert!(out.contains("Planning"), "got: {out}");
    assert!(out.contains("Q1 Goals"), "got: {out}");
}

#[test]
fn collect_json_titles_skips_non_string_title() {
    let json = br#"{"title":123,"other":"x"}"#;
    let mut out = String::new();
    collect_json_titles(json, &mut out, 100);
    assert!(out.is_empty(), "got: {out}");
}

#[test]
fn collect_json_titles_handles_escaped_quote() {
    let json = br#"{"title":"say \"hi\" here"}"#;
    let mut out = String::new();
    collect_json_titles(json, &mut out, 100);
    assert_eq!(out, "say \\\"hi\\\" here");
}

#[test]
fn collect_json_titles_respects_max_bytes() {
    let mut buf = Vec::new();
    for i in 0..20 {
        buf.extend_from_slice(format!("\"title\":\"verylongtitlevalue{i}\",").as_bytes());
    }
    let mut out = String::new();
    collect_json_titles(&buf, &mut out, 10);
    assert!(out.len() <= 10, "got len {}", out.len());
}

#[test]
fn find_subslice_finds_first_occurrence() {
    assert_eq!(find_subslice(b"abcXdef", b"X"), Some(3));
    assert_eq!(find_subslice(b"abc", b"zzz"), None);
    assert_eq!(find_subslice(b"", b"x"), None);
}

#[test]
fn millis_to_secs_rejects_sub_day_and_converts() {
    assert!(millis_to_secs(86_399_999).is_none());
    assert_eq!(millis_to_secs(86_400_000), Some(86_400));
    assert_eq!(millis_to_secs(1_700_000_000_000), Some(1_700_000_000));
}

#[test]
fn extract_text_uses_xml_and_json_fallback() {
    use std::io::{Cursor, Write};
    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in files {
            w.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start zip entry");
            w.write_all(data).expect("write zip entry");
        }
        w.finish().expect("finish zip").into_inner()
    }

    let xml_zip = zip_bytes(&[("content.xml", b"<a>from xml</a>")]);
    let mut reader = Cursor::new(xml_zip);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "from xml");

    let json_zip = zip_bytes(&[("content.json", br#"{"title":"from json"}"#)]);
    let mut reader = Cursor::new(json_zip);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "from json");
}

#[test]
fn extract_text_invalid_or_entryless_yields_empty() {
    use std::io::{Cursor, Write};
    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in files {
            w.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start zip entry");
            w.write_all(data).expect("write zip entry");
        }
        w.finish().expect("finish zip").into_inner()
    }

    let mut bad = Cursor::new(b"not a zip".to_vec());
    assert_eq!(extract_text(&mut bad, MIME_XMIND, 64), "");

    let empty = zip_bytes(&[("other.txt", b"x")]);
    let mut reader = Cursor::new(empty);
    assert_eq!(extract_text(&mut reader, MIME_XMIND, 64), "");
}
