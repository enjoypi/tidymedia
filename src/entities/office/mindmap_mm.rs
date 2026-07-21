//! `FreeMind` / `FreePlane` `.mm` XML 字节扫描 `<node CREATED="ms" MODIFIED="ms">`。
//!
//! `.mm` 是直接 XML 文件（不是 zip），根 `<map version="1.0.1">` 下 `<node CREATED="<u64>"
//! MODIFIED="<u64>" TEXT="..."/>`。`CREATED` / `MODIFIED` 是 Unix milliseconds（u64）。
//! 仅取根 `<node>`（首个匹配）—— 子节点的 CREATED/MODIFIED 是节点创建/修改时间不是文档时间。

use std::io::Read;

use crate::entities::backend::MediaReader;

const MM_SCAN_BYTES: usize = 64 * 1024;
const TAG_NODE_OPEN: &[u8] = b"<node";
const ATTR_CREATED: &[u8] = b"CREATED=\"";
const ATTR_MODIFIED: &[u8] = b"MODIFIED=\"";

/// 入口：读 reader 前 64 KB 后扫描首个 `<node>` 内 CREATED/MODIFIED。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let mut buf = Vec::with_capacity(MM_SCAN_BYTES);
    let mut limited = reader.take(MM_SCAN_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    extract_dates(&buf)
}

/// 纯字节扫描业务：找首个 `<node ...>` 后的 CREATED / MODIFIED 属性，millis 转 secs。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let Some(node_start) = find_subslice(buf, TAG_NODE_OPEN) else {
        return (0, 0);
    };
    let after = &buf[node_start + TAG_NODE_OPEN.len()..];
    // 限制扫描在 `<node` 后到 `>` 之间（属性段内）。
    let attr_end = find_byte(after, b'>').unwrap_or(after.len());
    let attrs = &after[..attr_end];
    let created = scan_quoted_u64(attrs, ATTR_CREATED)
        .and_then(millis_to_secs)
        .unwrap_or(0);
    let modified = scan_quoted_u64(attrs, ATTR_MODIFIED)
        .and_then(millis_to_secs)
        .unwrap_or(0);
    (created, modified)
}

/// 文本提取的读取上限：`.mm` 是纯 XML，节点文本在 `TEXT="..."` 属性。
const MM_TEXT_INPUT_CAP: usize = 256 * 1024;
const ATTR_TEXT: &[u8] = b"TEXT=\"";

/// 文本提取入口：读前 256 KiB 收集所有 `TEXT="..."` 属性值。
///
/// 整 fn `coverage(off)`：read 入口 Err arm 同 `parse`；业务由
/// `collect_text_attrs` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let mut buf = Vec::with_capacity(MM_TEXT_INPUT_CAP);
    let mut limited = reader.take(MM_TEXT_INPUT_CAP as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let mut out = String::new();
    collect_text_attrs(&buf, &mut out, max_bytes);
    out
}

/// 纯字节扫描业务：收集所有 `TEXT="..."` 属性值（XML 实体原样保留）。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn collect_text_attrs(buf: &[u8], out: &mut String, max_bytes: usize) {
    let mut pos = 0;
    while out.len() < max_bytes {
        let Some(rel) = find_subslice(&buf[pos..], ATTR_TEXT) else {
            break;
        };
        let start = pos + rel + ATTR_TEXT.len();
        let after = &buf[start..];
        let Some(end) = find_byte(after, b'"') else {
            break;
        };
        let val = &after[..end];
        if !val.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&String::from_utf8_lossy(val));
            super::scan::truncate_at_boundary(out, max_bytes);
        }
        pos = start + end + 1;
    }
}

/// 在 `attrs` 内找 `key`（`CREATED="`）后取直到 `"` 之间的 u64 数字。
#[cfg_attr(coverage_nightly, coverage(off))]
fn scan_quoted_u64(attrs: &[u8], key: &[u8]) -> Option<u64> {
    let pos = find_subslice(attrs, key)?;
    let after = &attrs[pos + key.len()..];
    let end = find_byte(after, b'"')?;
    let s = std::str::from_utf8(&after[..end]).ok()?;
    s.parse().ok()
}

/// Unix milliseconds → secs；负值或小到不算合理时间返 None。
#[cfg_attr(coverage_nightly, coverage(off))]
fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1000;
    // 至少 1970-01-02（早于此视为无效）。
    if secs < 86_400 { None } else { Some(secs) }
}

#[cfg_attr(coverage_nightly, coverage(off))]
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

#[cfg_attr(coverage_nightly, coverage(off))]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "mindmap_mm_tests.rs"]
mod tests;
