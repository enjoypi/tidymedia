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
