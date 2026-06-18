//! 思维导图 zip 容器解析。
//!
//! 首版仅支持 xmind 6+ `metadata.json`（`{"created": <ms>, "modified": <ms>}`）。
//! itmz / mindnode / mmap (`MindManager`) 子格式各有不同结构，YAGNI 后续 commit 接入。

use std::io::Read;

use serde::Deserialize;

use crate::entities::backend::MediaReader;

use super::{MIME_XMIND, MIME_XMIND_ALT};

const METADATA_JSON: &str = "metadata.json";
const JSON_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct XmindMetadata {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    modified: Option<u64>,
}

/// 入口：把 reader 当 zip 容器打开，按 mime 分流到子解析器。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return (0, 0);
    };
    if mime == MIME_XMIND || mime == MIME_XMIND_ALT {
        parse_xmind(&mut archive)
    } else {
        // itmz / mindnode / mmap (MindManager)：首版返 (0, 0) 让 mtime 兜底；
        // 后续 commit 按各格式接入。
        (0, 0)
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_xmind<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> (u64, u64) {
    let Ok(entry) = archive.by_name(METADATA_JSON) else {
        return (0, 0);
    };
    let mut content = Vec::with_capacity(JSON_MAX_BYTES);
    if entry
        .take(JSON_MAX_BYTES as u64)
        .read_to_end(&mut content)
        .is_err()
    {
        return (0, 0);
    }
    extract_dates_from_json(&content)
}

/// 纯 JSON 解析业务：取 `created` / `modified` 字段（milliseconds → secs）。
pub(super) fn extract_dates_from_json(buf: &[u8]) -> (u64, u64) {
    let Ok(meta) = serde_json::from_slice::<XmindMetadata>(buf) else {
        return (0, 0);
    };
    let created = meta.created.and_then(millis_to_secs).unwrap_or(0);
    let modified = meta.modified.and_then(millis_to_secs).unwrap_or(0);
    (created, modified)
}

fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1000;
    if secs < 86_400 { None } else { Some(secs) }
}

#[cfg(test)]
#[path = "mindmap_zip_tests.rs"]
mod tests;
