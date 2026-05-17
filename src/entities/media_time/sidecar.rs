// docs/media-time-detection.md §二.P3：旁路 sidecar。
// 仅识别两种常见格式，避免引入 XML 库：
//   - `<media>.xmp` 中的 `photoshop:DateCreated="<RFC3339>"`（纯文本搜索）
//   - Google Takeout `<media>.<ext>.json` 中的 `photoTakenTime.timestamp`（serde_json）

use std::sync::Arc;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use serde_derive::Deserialize;

use super::candidate::Candidate;
use super::priority::Source;
use crate::entities::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

const XMP_KEY: &str = "photoshop:DateCreated=\"";

/// 旧入口：本地路径 → Local backend shim。便于现有测试与 use case 不引入 backend 类型。
pub fn discover(media_path: &Utf8Path) -> Vec<Candidate> {
    let backend = LocalBackend::arc();
    discover_with_backend(&Location::Local(media_path.to_path_buf()), &backend)
}

/// Backend Gateway 入口：以 [`Location`] + [`Backend`] 在 backend 上读 sibling sidecar。
/// 当前 sibling 路径计算仅 Local 实现（[`with_extension`] / [`append_suffix`] 对非 Local
/// 返回 None），SMB/MTP 接入时再扩展。
pub fn discover_with_backend(
    media_loc: &Location,
    backend: &Arc<dyn Backend>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    if let Some(c) = try_xmp(media_loc, backend.as_ref()) {
        out.push(c);
    }
    if let Some(c) = try_takeout(media_loc, backend.as_ref()) {
        out.push(c);
    }
    out
}

fn try_xmp(media_loc: &Location, backend: &dyn Backend) -> Option<Candidate> {
    let xmp_loc = with_extension(media_loc, "xmp")?;
    let content = backend.read_to_string(&xmp_loc).ok()?;
    let utc = parse_xmp_date(&content)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::XmpSidecar,
        inferred_offset: false,
    })
}

fn try_takeout(media_loc: &Location, backend: &dyn Backend) -> Option<Candidate> {
    // Takeout 同目录文件：<media-full-name>.json（例如 photo.jpg.json）。
    // 直接拼后缀，避免 with_extension 在无扩展名时产生 `photo..json`（多一个点）。
    let json_loc = append_suffix(media_loc, ".json")?;
    let content = backend.read_to_string(&json_loc).ok()?;
    let utc = parse_takeout_json(&content)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::GoogleTakeoutJson,
        inferred_offset: false,
    })
}

/// 同 stem 替换扩展名。仅 Local case，其他 backend 暂返 None。
fn with_extension(loc: &Location, ext: &str) -> Option<Location> {
    let Location::Local(p) = loc else { return None };
    let mut pp = p.clone();
    pp.set_extension(ext);
    Some(Location::Local(pp))
}

/// 在原 path 末尾追加后缀，等价于 Takeout 的 `<media-full>.json`。
fn append_suffix(loc: &Location, sfx: &str) -> Option<Location> {
    let Location::Local(p) = loc else { return None };
    Some(Location::Local(Utf8PathBuf::from(format!("{p}{sfx}"))))
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
        let backend = LocalBackend::arc();
        assert!(try_xmp(&Location::Local(mp), backend.as_ref()).is_none());
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
        let backend = LocalBackend::arc();
        assert!(try_takeout(&Location::Local(mp), backend.as_ref()).is_none());
    }

    /// 非 Local backend：with_extension / append_suffix 都返回 None →
    /// discover_with_backend 直接返回空 Vec（Task 4 范围内 SMB/MTP 尚未支持 sibling）。
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

    /// FakeBackend 用 Local Location 喂入：read_to_string 走 fake 数据，验证 backend 调度
    /// 与 LocalBackend 同语义。
    #[test]
    fn discover_with_fake_backend_finds_xmp() {
        use crate::entities::backend::fake::FakeBackend;
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
}
