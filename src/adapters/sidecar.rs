//! Sidecar Gateway：识别 P3 旁路文件并把它们转成 [`Candidate`]（entities 时间候选）。
//! 仅识别两种常见格式，避免引入 XML 库：
//!   - `<media>.xmp` 中的 `photoshop:DateCreated="<RFC3339>"`（纯文本搜索）
//!   - Google Takeout `<media>.<ext>.json` 中的 `photoTakenTime.timestamp`（`serde_json`）
//!
//! `docs/media-time-detection.md` §二.P3。本模块属 Interface Adapters：把外部
//! sidecar 协议解析成内层 [`Candidate`]，protocol 细节（XMP 字面量 / Takeout schema /
//! `serde_json`）不泄漏到 entities / usecases。

use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use serde_derive::Deserialize;
use tracing::debug;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::media_time::Candidate;
use crate::entities::media_time::Source;
use crate::entities::uri::Location;
use crate::entities::xmp;

const FEATURE_SIDECAR: &str = "sidecar";

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
    let content = read_sidecar(&xmp_loc, backend, "read_xmp")?;
    let Some(utc) = parse_xmp_date(&content) else {
        log_parse_failure("parse_xmp", &xmp_loc);
        return None;
    };
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
    let content = read_sidecar(&json_loc, backend, "read_takeout")?;
    let Some(utc) = parse_takeout_json(&content) else {
        log_parse_failure("parse_takeout", &json_loc);
        return None;
    };
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

/// 读 sidecar 内容；失败时按需输出诊断日志（R3：外部读取不静默）。
fn read_sidecar(loc: &Location, backend: &dyn Backend, operation: &'static str) -> Option<String> {
    match backend.read_to_string(loc) {
        Ok(content) => Some(content),
        Err(e) => {
            if should_log_read_error(e.kind()) {
                let path = loc.display();
                let err = e.to_string();
                debug!(
                    feature = FEATURE_SIDECAR,
                    operation,
                    path,
                    result = "error",
                    err,
                    "cannot read sidecar"
                );
            }
            None
        }
    }
}

/// `NotFound` 是常态（绝大多数媒体没有 sidecar），记日志只会刷屏；其余 IO 错误才值得诊断。
fn should_log_read_error(kind: io::ErrorKind) -> bool {
    kind != io::ErrorKind::NotFound
}

/// sidecar 文件存在但内容不符合预期格式：这是 P3 候选"应生效而未生效"的关键诊断点。
fn log_parse_failure(operation: &'static str, loc: &Location) {
    let path = loc.display();
    debug!(
        feature = FEATURE_SIDECAR,
        operation,
        path,
        result = "error",
        "cannot parse sidecar"
    );
}

pub(crate) fn parse_xmp_date(content: &str) -> Option<DateTime<Utc>> {
    // P3 sidecar 沿用历史语义只取 photoshop:DateCreated。packet 嗅探 + 多键解析
    // 复用 entities::xmp 同一实现（避免 sidecar / EXIF fallback 两份 XML 解析漂移）。
    xmp::parse_xmp_dates(content)
        .photoshop_date_created
        .map(|dt| dt.with_timezone(&Utc))
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
