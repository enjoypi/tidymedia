//! Sidecar Gateway：识别 P3 旁路文件并把它们转成 [`Candidate`]（entities 时间候选）。
//! 仅识别两种常见格式，避免引入 XML 库：
//!   - `<media>.xmp` 中的 `photoshop:DateCreated="<RFC3339>"`（纯文本搜索）
//!   - Google Takeout `<media>.<ext>.json` 中的 `photoTakenTime.timestamp`（`serde_json`）
//!
//! `docs/media-time-detection.md` §二.P3。本模块属 Interface Adapters：把外部
//! sidecar 协议解析成内层 [`Candidate`]，protocol 细节（XMP 字面量 / Takeout schema /
//! `serde_json`）不泄漏到 entities / usecases。

use std::sync::Arc;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use serde_derive::Deserialize;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::media_time::Candidate;
use crate::entities::media_time::Source;
use crate::entities::uri::Location;

const XMP_KEY: &str = "photoshop:DateCreated=\"";

/// 旧入口：本地路径 → Local backend shim。便于现有测试与 use case 不引入 backend 类型。
#[must_use]
pub fn discover(media_path: &Utf8Path) -> Vec<Candidate> {
    let backend = LocalBackend::arc();
    discover_with_backend(&Location::Local(media_path.to_path_buf()), &backend)
}

/// Backend Gateway 入口：以 [`Location`] + [`Backend`] 在 backend 上读 sibling sidecar。
/// 当前 sibling 路径计算仅 Local 实现（[`with_extension`] / [`append_suffix`] 对非 Local
/// 返回 None），SMB/MTP 接入时再扩展。
pub fn discover_with_backend(media_loc: &Location, backend: &Arc<dyn Backend>) -> Vec<Candidate> {
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
    // XML 允许 <!-- ... --> 注释中包含与正文同形态 photoshop:DateCreated 字面量。
    // 直接 find 首次出现会被注释干扰；先把所有注释体替换为同长度空白，再搜索。
    let stripped = strip_xml_comments(content);
    let key_idx = stripped.find(XMP_KEY)?;
    let start = key_idx + XMP_KEY.len();
    let rest = &stripped[start..];
    let end = rest.find('"')?;
    let raw = &rest[..end];
    let dt = DateTime::parse_from_rfc3339(raw).ok()?;
    Some(dt.with_timezone(&Utc))
}

// 把 `<!-- ... -->` 注释体替换为同字节数的空格，保持偏移与原串一致。
// XML 注释不可嵌套，按 `<!--`/`-->` 线性配对。注释边界全 ASCII，操作不破坏 UTF-8。
fn strip_xml_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<!--") {
            let body_start = i + 4;
            let end = bytes
                .get(body_start..)
                .and_then(|tail| tail.windows(3).position(|w| w == b"-->"))
                .map_or(bytes.len(), |p| body_start + p + 3);
            for _ in i..end {
                out.push(' ');
            }
            i = end;
        } else {
            // 非注释字节按 UTF-8 字符边界推进，避免切到多字节字符内部。
            let ch_end = (i + 1..=bytes.len())
                .find(|&j| content.is_char_boundary(j))
                .unwrap_or(bytes.len());
            out.push_str(&content[i..ch_end]);
            i = ch_end;
        }
    }
    out
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
    // try_seconds 越界返 None；DateTime + TimeDelta 仍可能溢出，用 checked_add_signed 兜底。
    // 防止外部数据 (任意大整数 ≤ i64::MAX) 在 sidecar::discover 公开入口触发 panic。
    let delta = TimeDelta::try_seconds(secs)?;
    DateTime::<Utc>::UNIX_EPOCH.checked_add_signed(delta)
}

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod tests;
