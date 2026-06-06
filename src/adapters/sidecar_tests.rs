use camino::Utf8PathBuf;

use super::*;

#[test]
fn parse_xmp_date_ok() {
    let xml = r#"<rdf:Description photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>"#;
    let utc = parse_xmp_date(xml).unwrap();
    // 14:30 +08:00 = 06:30 UTC
    assert_eq!(utc.timestamp(), 1_714_545_000);
}

#[test]
fn parse_xmp_date_missing_key_none() {
    assert!(parse_xmp_date("no key here").is_none());
}

#[test]
fn parse_xmp_date_unterminated_quote_none() {
    assert!(parse_xmp_date(r#"photoshop:DateCreated="2024-05-01T14:30:00"#).is_none());
}

#[test]
fn parse_xmp_date_invalid_rfc3339_none() {
    assert!(parse_xmp_date(r#"photoshop:DateCreated="not a date""#).is_none());
}

/// content 恰好以 KEY 结尾 → start == `content.len()` → rest 为空字符串
/// → find('"') None → 整体 None。
#[test]
fn parse_xmp_date_key_at_end_returns_none() {
    let s = format!("aaa{XMP_KEY}");
    assert!(parse_xmp_date(&s).is_none());
}

#[test]
fn parse_takeout_json_ok() {
    let s = r#"{"photoTakenTime":{"timestamp":"1714576200","formatted":"..."}}"#;
    let utc = parse_takeout_json(s).unwrap();
    assert_eq!(utc.timestamp(), 1_714_576_200);
}

#[test]
fn parse_takeout_json_missing_field_none() {
    assert!(parse_takeout_json(r#"{"other":"data"}"#).is_none());
}

#[test]
fn parse_takeout_json_invalid_timestamp_none() {
    assert!(parse_takeout_json(r#"{"photoTakenTime":{"timestamp":"abc"}}"#).is_none());
}

/// 越界但合法 i64 的 timestamp 不应 panic：`TimeDelta::try_seconds` 返 None → 整体 None。
/// 1e18 > chrono `TimeDelta::MAX` (≈ `i64::MAX` / 1000)。
#[test]
fn parse_takeout_json_overflow_returns_none() {
    let s = r#"{"photoTakenTime":{"timestamp":"1000000000000000000"}}"#;
    assert!(parse_takeout_json(s).is_none());
}

/// 直测 `strip_xml_comments` 精确输出：注释体替换为同字节数空格、前后正文与偏移
/// 原样保留。注释起点远离 0（i=5）能区分 `i+4` 被变异成 `i*4`（越界→整段误抹）、
/// `p+3` 变异成 `p-3`（尾部 `-->` 残留）等算术错误——仅断言解析结果杀不掉这些变异。
#[test]
fn strip_xml_comments_blanks_mid_text_comment_preserving_offsets() {
    assert_eq!(
        strip_xml_comments("12345<!--x-->after"),
        "12345        after"
    );
}

/// 真实场景下注释先于属性出现：`parse_xmp_date` 应解析正文属性而非注释里的字面量。
#[test]
fn parse_xmp_date_skips_xml_comment() {
    let xml = r#"<!-- example photoshop:DateCreated="2020-01-01T00:00:00Z" -->
<rdf:Description photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>"#;
    let utc = parse_xmp_date(xml).unwrap();
    // 应取注释外的 2024-05-01 14:30 +08:00 = 06:30 UTC，而非注释中的 2020-01-01。
    assert_eq!(utc.timestamp(), 1_714_545_000);
}

/// 注释未闭合（缺 `-->`）时所有剩余内容均按注释体处理 → 整体不应找到 KEY → None。
#[test]
fn parse_xmp_date_unterminated_comment_none() {
    let xml = r#"<!-- photoshop:DateCreated="2024-05-01T14:30:00Z" "#;
    assert!(parse_xmp_date(xml).is_none());
}

/// 注释体含多字节 UTF-8 字符（如中文）也不应破坏字符边界。
#[test]
fn parse_xmp_date_comment_with_multibyte_chars() {
    let xml = "<!-- 中文示例 photoshop:DateCreated=\"2020-01-01T00:00:00Z\" -->\
<rdf:Description photoshop:DateCreated=\"2024-05-01T14:30:00+08:00\"/>";
    let utc = parse_xmp_date(xml).unwrap();
    assert_eq!(utc.timestamp(), 1_714_545_000);
}

#[test]
fn discover_finds_xmp_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("x.jpg");
    std::fs::write(&media, b"jpg-bytes").unwrap();
    let xmp = dir.path().join("x.xmp");
    std::fs::write(&xmp, r#"photoshop:DateCreated="2024-05-01T14:30:00+08:00""#).unwrap();

    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    let cands = discover(&mp);
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, Source::XmpSidecar);
}

#[test]
fn discover_finds_takeout_json_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("photo.jpg");
    std::fs::write(&media, b"jpg-bytes").unwrap();
    let json = dir.path().join("photo.jpg.json");
    std::fs::write(&json, r#"{"photoTakenTime":{"timestamp":"1714576200"}}"#).unwrap();

    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    let cands = discover(&mp);
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, Source::GoogleTakeoutJson);
    assert_eq!(cands[0].utc.timestamp(), 1_714_576_200);
}

#[test]
fn discover_returns_empty_when_no_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("lone.jpg");
    std::fs::write(&media, b"jpg-bytes").unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    assert!(discover(&mp).is_empty());
}

/// xmp 文件存在但内容无法解析 → `parse_xmp_date` None → `try_xmp` None
#[test]
fn try_xmp_unparseable_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("bad.jpg");
    std::fs::write(&media, b"x").unwrap();
    let xmp = dir.path().join("bad.xmp");
    std::fs::write(&xmp, b"not xmp content").unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    let backend = LocalBackend::arc();
    assert!(try_xmp(&Location::Local(mp), backend.as_ref()).is_none());
}

/// json 文件存在但内容不符合 schema → `try_takeout` None
#[test]
fn try_takeout_unparseable_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("bad.jpg");
    std::fs::write(&media, b"x").unwrap();
    let json = dir.path().join("bad.jpg.json");
    std::fs::write(&json, b"{}").unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    let backend = LocalBackend::arc();
    assert!(try_takeout(&Location::Local(mp), backend.as_ref()).is_none());
}

/// 非 Local `backend：with_extension` / `append_suffix` 都返回 None →
/// `discover_with_backend` 直接返回空 Vec（SMB/MTP/ADB 暂未支持 sibling 探测；
/// 见 docs/media-time-detection.md 末「Known limitations」）。
#[test]
fn discover_with_backend_smb_returns_empty() {
    let smb_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("dir/x.jpg"),
    };
    let backend = LocalBackend::arc();
    assert!(discover_with_backend(&smb_loc, &backend).is_empty());
}

/// ADB Location 走同一非 Local guard，应直接返回空 Vec。
#[test]
fn discover_with_backend_adb_returns_empty() {
    let adb_loc = Location::Adb {
        serial: None,
        path: Utf8PathBuf::from("/sdcard/DCIM/x.jpg"),
    };
    let backend = LocalBackend::arc();
    assert!(discover_with_backend(&adb_loc, &backend).is_empty());
}

/// MTP Location 同样走非 Local guard。
#[test]
fn discover_with_backend_mtp_returns_empty() {
    let mtp_loc = Location::Mtp {
        device: "phone".into(),
        storage: "internal".into(),
        path: Utf8PathBuf::from("DCIM/Camera/x.jpg"),
    };
    let backend = LocalBackend::arc();
    assert!(discover_with_backend(&mtp_loc, &backend).is_empty());
}

/// `NotFound` 是 sidecar 缺失常态，不值得记日志；其余 IO 错误（超时/权限）才需诊断。
#[test]
fn should_log_read_error_skips_not_found() {
    assert!(!should_log_read_error(std::io::ErrorKind::NotFound));
}

#[test]
fn should_log_read_error_reports_timed_out() {
    assert!(should_log_read_error(std::io::ErrorKind::TimedOut));
}

/// sidecar 读取遇非 `NotFound` 错误（注入 `TimedOut`）：走日志分支后仍返回空候选，
/// 不让 P3 失败中断扫描。
#[test]
fn discover_with_fake_backend_read_error_returns_empty() {
    use crate::adapters::backend::fake::{FakeBackend, Op};
    let fake = std::sync::Arc::new(FakeBackend::new("local"));
    let media = Location::Local(Utf8PathBuf::from("/in-mem/x.jpg"));
    let xmp = Location::Local(Utf8PathBuf::from("/in-mem/x.xmp"));
    let json = Location::Local(Utf8PathBuf::from("/in-mem/x.jpg.json"));
    fake.add_file(media.clone(), b"img-bytes".to_vec());
    fake.inject_error(xmp, Op::ReadToString, std::io::ErrorKind::TimedOut);
    fake.inject_error(json, Op::ReadToString, std::io::ErrorKind::TimedOut);
    let backend: std::sync::Arc<dyn Backend> = fake;
    assert!(discover_with_backend(&media, &backend).is_empty());
}

/// `FakeBackend` 用 Local Location `喂入：read_to_string` 走 fake 数据，验证 backend 调度
/// 与 `LocalBackend` 同语义。
#[test]
fn discover_with_fake_backend_finds_xmp() {
    use crate::adapters::backend::fake::FakeBackend;
    let fake = std::sync::Arc::new(FakeBackend::new("local"));
    let media = Location::Local(Utf8PathBuf::from("/in-mem/x.jpg"));
    let xmp = Location::Local(Utf8PathBuf::from("/in-mem/x.xmp"));
    fake.add_file(media.clone(), b"img-bytes".to_vec());
    fake.add_file(
        xmp,
        br#"photoshop:DateCreated="2024-05-01T14:30:00+00:00""#.to_vec(),
    );
    let backend: std::sync::Arc<dyn Backend> = fake;
    let cands = discover_with_backend(&media, &backend);
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, Source::XmpSidecar);
}
