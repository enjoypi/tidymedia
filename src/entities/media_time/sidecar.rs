// docs/media-time-detection.md §二.P3：旁路 sidecar。
// 仅识别两种常见格式，避免引入 XML 库：
//   - `<media>.xmp` 中的 `photoshop:DateCreated="<RFC3339>"`（纯文本搜索）
//   - Google Takeout `<media>.<ext>.json` 中的 `photoTakenTime.timestamp`（serde_json）

use std::fs;

use camino::Utf8Path;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use serde_derive::Deserialize;

use super::candidate::Candidate;
use super::priority::Source;

const XMP_KEY: &str = "photoshop:DateCreated=\"";

/// 同一个媒体文件可能伴随多个 sidecar；按出现顺序返回所有 P3 候选。
pub fn discover(media_path: &Utf8Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    if let Some(c) = try_xmp(media_path) {
        out.push(c);
    }
    if let Some(c) = try_takeout(media_path) {
        out.push(c);
    }
    out
}

fn try_xmp(media_path: &Utf8Path) -> Option<Candidate> {
    // 同目录同 stem 的 .xmp（spec §2.P3）
    let mut xmp = media_path.to_path_buf();
    xmp.set_extension("xmp");
    let content = fs::read_to_string(xmp.as_std_path()).ok()?;
    let utc = parse_xmp_date(&content)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::XmpSidecar,
        inferred_offset: false,
    })
}

fn try_takeout(media_path: &Utf8Path) -> Option<Candidate> {
    // Takeout 同目录文件：<media-full-name>.json（例如 photo.jpg.json）。
    // 直接拼后缀，避免 with_extension 在无扩展名时产生 `photo..json`（多一个点）。
    let json_path = Utf8Path::new(&format!("{}.json", media_path.as_str())).to_path_buf();
    let content = fs::read_to_string(json_path.as_std_path()).ok()?;
    let utc = parse_takeout_json(&content)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::GoogleTakeoutJson,
        inferred_offset: false,
    })
}

pub(crate) fn parse_xmp_date(content: &str) -> Option<DateTime<Utc>> {
    let key_idx = content.find(XMP_KEY)?;
    // `find` 保证 key_idx + KEY.len() ≤ content.len()，因此 [start..] 安全；
    // 避免再加一层 `content.get(..)?` 的不可达 Err 分支。
    let start = key_idx + XMP_KEY.len();
    let rest = &content[start..];
    let end = rest.find('"')?;
    let raw = &rest[..end];
    let dt = DateTime::parse_from_rfc3339(raw).ok()?;
    Some(dt.with_timezone(&Utc))
}

#[derive(Deserialize)]
struct TakeoutTime {
    timestamp: String,
}

#[derive(Deserialize)]
struct TakeoutEnvelope {
    #[serde(rename = "photoTakenTime")]
    photo_taken_time: TakeoutTime,
}

pub(crate) fn parse_takeout_json(content: &str) -> Option<DateTime<Utc>> {
    let env: TakeoutEnvelope = serde_json::from_str(content).ok()?;
    let secs: i64 = env.photo_taken_time.timestamp.parse().ok()?;
    Some(DateTime::<Utc>::UNIX_EPOCH + TimeDelta::seconds(secs))
}

#[cfg(test)]
mod tests {
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

    /// content 恰好以 KEY 结尾 → start == content.len() → rest 为空字符串
    /// → find('"') None → 整体 None。
    #[test]
    fn parse_xmp_date_key_at_end_returns_none() {
        let s = format!("aaa{}", XMP_KEY);
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
        std::fs::write(
            &json,
            r#"{"photoTakenTime":{"timestamp":"1714576200"}}"#,
        )
        .unwrap();

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

    /// xmp 文件存在但内容无法解析 → parse_xmp_date None → try_xmp None
    #[test]
    fn try_xmp_unparseable_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("bad.jpg");
        std::fs::write(&media, b"x").unwrap();
        let xmp = dir.path().join("bad.xmp");
        std::fs::write(&xmp, b"not xmp content").unwrap();
        let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
        assert!(try_xmp(&mp).is_none());
    }

    /// json 文件存在但内容不符合 schema → try_takeout None
    #[test]
    fn try_takeout_unparseable_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("bad.jpg");
        std::fs::write(&media, b"x").unwrap();
        let json = dir.path().join("bad.jpg.json");
        std::fs::write(&json, b"{}").unwrap();
        let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
        assert!(try_takeout(&mp).is_none());
    }
}
