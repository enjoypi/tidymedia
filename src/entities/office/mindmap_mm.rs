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

/// 在 `attrs` 内找 `key`（`CREATED="`）后取直到 `"` 之间的 u64 数字。
fn scan_quoted_u64(attrs: &[u8], key: &[u8]) -> Option<u64> {
    let pos = find_subslice(attrs, key)?;
    let after = &attrs[pos + key.len()..];
    let end = find_byte(after, b'"')?;
    let s = std::str::from_utf8(&after[..end]).ok()?;
    s.parse().ok()
}

/// Unix milliseconds → secs；负值或小到不算合理时间返 None。
fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1000;
    // 至少 1970-01-02（早于此视为无效）。
    if secs < 86_400 { None } else { Some(secs) }
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
#[path = "mindmap_mm_tests.rs"]
mod tests;
