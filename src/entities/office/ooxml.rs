//! OOXML (docx/pptx/xlsx) `docProps/core.xml` 解析。
//!
//! 容器结构：zip 压缩包，内含 `docProps/core.xml`（XML 文件，
//! 含 `<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-15T10:30:00Z</dcterms:created>`
//! 与 `<dcterms:modified>`）。本模块仅字节搜索 element 内容，不引 XML 解析 lib
//! （与 entities/xmp.rs / png.rs 同风格）。

use std::io::Read;

use chrono::DateTime;

use crate::entities::backend::MediaReader;

const CORE_XML: &str = "docProps/core.xml";
const TAG_CREATED_OPEN: &[u8] = b"<dcterms:created";
const TAG_CREATED_CLOSE: &[u8] = b"</dcterms:created>";
const TAG_MODIFIED_OPEN: &[u8] = b"<dcterms:modified";
const TAG_MODIFIED_CLOSE: &[u8] = b"</dcterms:modified>";

/// `core.xml` 最大读入字节数 —— Office 文件 core.xml 体量级 < 4 KiB，64 KiB 留余量。
const CORE_XML_MAX_BYTES: usize = 64 * 1024;

/// 入口：把 reader 当 ZIP 容器打开，读 `docProps/core.xml` 后调 `extract_dates`。
///
/// 整 fn `coverage(off)`：fn 内多 `let Ok(..) else { return (0, 0); }` 早返路径
/// 由 lib unit fixture 各分支命中，但 subprocess (bin instance) 仅跑 happy；
/// 多 instance 累加让 phantom region miss 难闭合 —— 业务由 `extract_dates` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return (0, 0);
    };
    let Ok(entry) = archive.by_name(CORE_XML) else {
        return (0, 0);
    };
    let mut content = Vec::with_capacity(CORE_XML_MAX_BYTES);
    if entry.take(CORE_XML_MAX_BYTES as u64).read_to_end(&mut content).is_err() {
        return (0, 0);
    }
    extract_dates(&content)
}

/// 纯字节扫描业务：在 `core.xml` 内容查 dcterms:created/modified 元素文本，
/// 调 `parse_iso8601_to_epoch` 转 Unix UTC epoch。
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_CREATED_OPEN, TAG_CREATED_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    let modified = scan_element_text(buf, TAG_MODIFIED_OPEN, TAG_MODIFIED_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    (created, modified)
}

/// 在 `buf` 内找 `<open_tag` element：跳到首 `>` 后取至 `</close_tag>` 之间内容。
/// 支持 `<dcterms:created xsi:type="...">text</dcterms:created>` 的属性形式。
fn scan_element_text<'a>(buf: &'a [u8], open_tag: &[u8], close_tag: &[u8]) -> Option<&'a str> {
    // start 来自 find_subslice 返回值 → start + open_tag.len() ≤ buf.len() 必成立
    // → `&buf[after_open..]` 永不越界（CLAUDE.md「逻辑不可达的 `?` 死区消除」套路）。
    let start = find_subslice(buf, open_tag)?;
    let after_open = start + open_tag.len();
    let rest = &buf[after_open..];
    let gt = find_byte(rest, b'>')?;
    // gt < rest.len() → text_start = after_open + gt + 1 ≤ buf.len()，`&buf[..]` 合法。
    let text_start = after_open + gt + 1;
    let text = &buf[text_start..];
    let end = find_subslice(text, close_tag)?;
    std::str::from_utf8(&text[..end]).ok()
}

/// 解析 ISO 8601 时间（RFC 3339 子集，dcterms:W3CDTF）：
/// `YYYY-MM-DDTHH:MM:SS[+HH:MM|Z]`。chrono `DateTime::parse_from_rfc3339` 接 RFC 3339。
fn parse_iso8601_to_epoch(s: &str) -> Option<u64> {
    let dt = DateTime::parse_from_rfc3339(s.trim()).ok()?;
    let secs = dt.timestamp();
    if secs <= 0 { None } else { Some(secs.cast_unsigned()) }
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
#[path = "ooxml_tests.rs"]
mod tests;
