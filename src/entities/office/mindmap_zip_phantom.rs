//! 思维导图 zip 解析入口与复杂业务 fn。
//!
//! 这些 fn 被 `OFFICE_FIXTURES` bin subprocess / `office_deep` 集成测试命中时只走
//! happy path，变体分支在对应实例计 0（per-instance phantom branch 缺口）。无法经
//! 测试补齐，按「函数独立 + ignore-regex 排除」处理（CLAUDE.md office 子模块套路）。

use std::io::{Read, Seek};

use crate::entities::backend::MediaReader;
use crate::entities::office::scan::{strip_markup_into, truncate_at_boundary};

use super::{
    CONTENT_JSON, CONTENT_MAX_BYTES, CONTENT_XML, JSON_MAX_BYTES, METADATA_JSON, MIME_XMIND,
    MIME_XMIND_ALT, XmindMetadata,
};

/// 入口：把 reader 当 zip 容器打开，按 mime 分流到子解析器。
#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
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

#[doc(hidden)]
pub fn parse_xmind<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> (u64, u64) {
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

/// 文本提取入口：优先 `content.xml`（剥 XML 标签），缺失则 `content.json`
/// （抓 `"title":"..."` 值）；itmz/mindnode/mmap 无已知入口返空（同 `parse` 限制）。
///
/// zip 打开/entry 缺失早返路径同 `parse`；业务由 `scan::strip_markup_into` /
/// `collect_json_titles` 单测真测。
#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(xml) = read_entry_capped(&mut archive, CONTENT_XML) {
        strip_markup_into(&xml, &mut out, max_bytes);
    } else if let Some(json) = read_entry_capped(&mut archive, CONTENT_JSON) {
        collect_json_titles(&json, &mut out, max_bytes);
    }
    out
}

#[doc(hidden)]
pub fn read_entry_capped<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    let Ok(entry) = archive.by_name(name) else {
        return None;
    };
    let mut content = Vec::new();
    if entry
        .take(CONTENT_MAX_BYTES)
        .read_to_end(&mut content)
        .is_err()
    {
        return None;
    }
    Some(content)
}

/// 纯字节扫描业务：收集 JSON 内所有 `"title":"..."` 值（xmind zen topic 文本）。
/// 不整树反序列化——只需要文本片段喂分类器。
#[doc(hidden)]
pub fn collect_json_titles(buf: &[u8], out: &mut String, max_bytes: usize) {
    const KEY: &[u8] = b"\"title\":";
    let mut pos = 0;
    while out.len() < max_bytes {
        let Some(rel) = find_subslice(&buf[pos..], KEY) else {
            break;
        };
        let mut i = pos + rel + KEY.len();
        while i < buf.len() && buf[i].is_ascii_whitespace() {
            i += 1;
        }
        if buf.get(i) != Some(&b'"') {
            pos = i;
            continue;
        }
        i += 1;
        let start = i;
        while i < buf.len() && buf[i] != b'"' {
            if buf[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        let val = &buf[start..i.min(buf.len())];
        if !val.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&String::from_utf8_lossy(val));
            truncate_at_boundary(out, max_bytes);
        }
        pos = i;
    }
}

#[doc(hidden)]
#[must_use]
pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 纯 JSON 解析业务：取 `created` / `modified` 字段（milliseconds → secs）。
#[doc(hidden)]
pub fn extract_dates_from_json(buf: &[u8]) -> (u64, u64) {
    let Ok(meta) = serde_json::from_slice::<XmindMetadata>(buf) else {
        return (0, 0);
    };
    let created = meta.created.and_then(millis_to_secs).unwrap_or(0);
    let modified = meta.modified.and_then(millis_to_secs).unwrap_or(0);
    (created, modified)
}

#[doc(hidden)]
#[must_use]
pub fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1000;
    if secs < 86_400 { None } else { Some(secs) }
}
