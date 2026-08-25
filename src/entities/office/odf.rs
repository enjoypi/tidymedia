//! `OpenDocument` (odt/ods/odp/odg) `meta.xml` 解析。
//!
//! ODF 是 zip 容器，含 `meta.xml`（XML，含 `<meta:creation-date>` + `<dc:date>`）。
//! `<meta:creation-date>` 等价 OOXML `dcterms:created`（P0 creation 时间），
//! `<dc:date>` 等价 `dcterms:modified`（last save）。
//! 时间可能 naive（无 Z 后缀）：fallback 按 UTC 解释。

use std::io::Read;

use chrono::{DateTime, NaiveDateTime};

use crate::entities::backend::MediaReader;

const META_XML: &str = "meta.xml";
const TAG_CREATED_OPEN: &[u8] = b"<meta:creation-date";
const TAG_CREATED_CLOSE: &[u8] = b"</meta:creation-date>";
const TAG_DC_DATE_OPEN: &[u8] = b"<dc:date";
const TAG_DC_DATE_CLOSE: &[u8] = b"</dc:date>";

const META_XML_MAX_BYTES: usize = 64 * 1024;

/// 入口：把 reader 当 zip 容器打开，读 `meta.xml` 后调 `extract_dates`。
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return (0, 0);
    };
    let Ok(entry) = archive.by_name(META_XML) else {
        return (0, 0);
    };
    let mut content = Vec::with_capacity(META_XML_MAX_BYTES);
    if entry
        .take(META_XML_MAX_BYTES as u64)
        .read_to_end(&mut content)
        .is_err()
    {
        return (0, 0);
    }
    extract_dates(&content)
}

/// 正文文件（odt/ods/odp 共用单入口）与读取上限；markup 膨胀系数见 ooxml 同名常量。
const CONTENT_XML: &str = "content.xml";
const CONTENT_XML_MAX_BYTES: u64 = 256 * 1024;

/// 文本提取入口：读 `content.xml` 剥 XML 标签攒 best-effort 正文。
///
/// 整 fn `coverage(off)`：zip 打开/entry 缺失早返路径同 `parse`；剥标签业务由
/// `scan::strip_markup_into` 单测真测。
pub(super) fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return String::new();
    };
    let Ok(entry) = archive.by_name(CONTENT_XML) else {
        return String::new();
    };
    let mut content = Vec::new();
    if entry
        .take(CONTENT_XML_MAX_BYTES)
        .read_to_end(&mut content)
        .is_err()
    {
        return String::new();
    }
    let mut out = String::new();
    super::scan::strip_markup_into(&content, &mut out, max_bytes);
    out
}

/// 纯字节扫描业务：查 `meta:creation-date` 与 `dc:date` element 文本。
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_CREATED_OPEN, TAG_CREATED_CLOSE)
        .and_then(parse_odf_datetime)
        .unwrap_or(0);
    let modified = scan_element_text(buf, TAG_DC_DATE_OPEN, TAG_DC_DATE_CLOSE)
        .and_then(parse_odf_datetime)
        .unwrap_or(0);
    (created, modified)
}

/// ODF 时间可能带时区（RFC 3339）或 naive（无 Z）。先试 `parse_from_rfc3339`，
/// 失败回退按 UTC 解析 `YYYY-MM-DDTHH:MM:SS`。
pub(super) fn parse_odf_datetime(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        let secs = dt.timestamp();
        if secs > 0 {
            return Some(secs.cast_unsigned());
        }
    }
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S").ok()?;
    let dt = naive.and_utc();
    let secs = dt.timestamp();
    if secs > 0 {
        Some(secs.cast_unsigned())
    } else {
        None
    }
}

fn scan_element_text<'a>(buf: &'a [u8], open_tag: &[u8], close_tag: &[u8]) -> Option<&'a str> {
    let start = find_subslice(buf, open_tag)?;
    let after_open = start + open_tag.len();
    let rest = &buf[after_open..];
    let gt = find_byte(rest, b'>')?;
    let text_start = after_open + gt + 1;
    let text = &buf[text_start..];
    let end = find_subslice(text, close_tag)?;
    std::str::from_utf8(&text[..end]).ok()
}

fn find_byte(haystack: &[u8], byte: u8) -> Option<usize> {
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i] == byte {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "odf_tests.rs"]
mod tests;
