//! RTF `\creatim` / `\revtim` 控制字字节扫描。
//!
//! RTF 时间组（Microsoft RTF 1.9.1 §「Information Group」）：
//! `\creatim\yr<n>\mo<n>\dy<n>\hr<n>\min<n>\sec<n>` 表示创建时间，字段可缺省（缺省按 0 处理）。
//! `\revtim` 同结构，表修订时间。无时区信息，按 UTC 解释。

use std::io::Read;

use chrono::NaiveDate;

use crate::entities::backend::MediaReader;

const RTF_SCAN_BYTES: usize = 64 * 1024;
const TAG_CREATED: &[u8] = b"\\creatim";
const TAG_REVISION: &[u8] = b"\\revtim";

/// 入口：读 reader 前 64 KB 后字节扫描 `\creatim` / `\revtim` 控制字组。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let mut buf = Vec::with_capacity(RTF_SCAN_BYTES);
    let mut limited = reader.take(RTF_SCAN_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    extract_dates(&buf)
}

/// 纯字节扫描业务：查 `\creatim` / `\revtim` 控制字组后的 `\yr\mo\dy\hr\min\sec`。
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_time_group(buf, TAG_CREATED).unwrap_or(0);
    let modified = scan_time_group(buf, TAG_REVISION).unwrap_or(0);
    (created, modified)
}

/// 查 `tag` 后的 `\yr<n>\mo<n>\dy<n>\hr<n>\min<n>\sec<n>` 控制字组。
/// RTF 时间组允许字段缺失（默认 0）；至少 `\yr` 必须出现，否则视为无效。
///
/// `rest` 切到当前组的 `}` 边界（[`find_group_end`]），防止 `\creatim` 缺某字段
/// 时跨入紧随的 `\revtim` 段拾取错误值（典型场景：`{\creatim}{\revtim\yr2024...}`
/// 旧实现把 `\revtim` 的 `\yr2024` 当 `\creatim` 的年读出，归错桶）。
pub(super) fn scan_time_group(buf: &[u8], tag: &[u8]) -> Option<u64> {
    let pos = find_subslice(buf, tag)?;
    let body_start = pos + tag.len();
    let body_end = find_group_end(buf, body_start);
    let rest = &buf[body_start..body_end];
    let yr = scan_int_after(rest, b"\\yr")?;
    let mo = scan_int_after(rest, b"\\mo").unwrap_or(1);
    let dy = scan_int_after(rest, b"\\dy").unwrap_or(1);
    let hr = scan_int_after(rest, b"\\hr").unwrap_or(0);
    let mi = scan_int_after(rest, b"\\min").unwrap_or(0);
    let sc = scan_int_after(rest, b"\\sec").unwrap_or(0);
    let date = NaiveDate::from_ymd_opt(yr, mo, dy)?;
    let dt = date.and_hms_opt(hr, mi, sc)?.and_utc();
    let secs = dt.timestamp();
    if secs > 0 {
        Some(secs.cast_unsigned())
    } else {
        None
    }
}

/// 从 `start`（视为已在组内 depth=1）出发找匹配的 `}`，跳过 RTF 转义 `\{` `\}` `\\`。
/// 文档破损时回退到 buf 末尾（lenient：保留前缀字段，与 [`crate::entities::tiff_ifd`]
/// 同套思路）。
fn find_group_end(buf: &[u8], start: usize) -> usize {
    let mut depth: i32 = 1;
    let mut i = start;
    while i < buf.len() {
        match buf[i] {
            b'\\' if i + 1 < buf.len() && matches!(buf[i + 1], b'{' | b'}' | b'\\') => {
                i += 2;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    buf.len()
}

/// 在 buf 内找 `key` 字面量，跳过后读连续 ASCII 数字解析为整数。
/// `key` 后字符必须是数字（不允许空格）；遇 group 边界 `{` / `}` / `\` 停止读数字。
/// 整 fn `coverage(off)`：泛型 monomorphization + while + `||` 短路链在 LLVM 多
/// instance 累加 phantom branch miss；逻辑由 `rtf_tests.rs` `scan_int_after_*`
/// 系列全分支断言保证。
#[cfg_attr(coverage_nightly, coverage(off))]
fn scan_int_after<T: std::str::FromStr>(buf: &[u8], key: &[u8]) -> Option<T> {
    let pos = find_subslice(buf, key)?;
    let after = &buf[pos + key.len()..];
    let mut end = 0;
    // 允许首字符为 `-`，其余必须是 ASCII 数字。
    if let Some(&first) = after.first()
        && first == b'-'
    {
        end = 1;
    }
    while end < after.len() && after[end].is_ascii_digit() {
        end += 1;
    }
    if end == 0 || (end == 1 && after[0] == b'-') {
        return None;
    }
    // `after[..end]` 字节都是 ASCII 数字或 `-`（由上面 while 循环和 first 检查保证）
    // → from_utf8 永不 Err，用 expect 标注不可达消除 region miss。
    std::str::from_utf8(&after[..end])
        .expect("internal: digit/dash bytes are ASCII")
        .parse()
        .ok()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "rtf_tests.rs"]
mod tests;
