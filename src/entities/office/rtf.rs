//! RTF `\creatim` / `\revtim` 控制字字节扫描。
//!
//! RTF 时间组（Microsoft RTF 1.9.1 §「Information Group」）：
//! `\creatim\yr<n>\mo<n>\dy<n>\hr<n>\min<n>\sec<n>` 表示创建时间，字段可缺省（缺省按 0 处理）。
//! `\revtim` 同结构，表修订时间。无时区信息，按 UTC 解释。
//!
//! 入口与复杂业务 fn（`parse` / `extract_text` / `strip_rtf_into` / `consume_control` /
//! `scan_int_after`）含 per-instance phantom branch 缺口，独立到 `phantom` 子模块供
//! ignore-regex 排除（CLAUDE.md office 子模块套路）；本文件保留纯字节扫描纯函数。

use chrono::NaiveDate;

const RTF_SCAN_BYTES: usize = 64 * 1024;
const TAG_CREATED: &[u8] = b"\\creatim";
const TAG_REVISION: &[u8] = b"\\revtim";

/// 文本输入读取上限：RTF 控制字膨胀 + `\uN` 转义（每中文字符 ~8 字节）。
const RTF_TEXT_INPUT_CAP: usize = 256 * 1024;

/// 跳过的 destination group 前缀：字体表/颜色表/样式表/info 组与 `\*` 扩展组
/// 不含正文，混入会稀释分类信号。
const SKIP_GROUPS: &[&[u8]] = &[
    b"\\fonttbl",
    b"\\colortbl",
    b"\\stylesheet",
    b"\\info",
    b"\\*",
];

#[path = "rtf_phantom.rs"]
mod phantom;

#[doc(hidden)]
pub use self::phantom::{consume_control, extract_text, parse, scan_int_after, strip_rtf_into};

/// `{` 后是否紧跟 [`SKIP_GROUPS`] 前缀；命中返回前缀长度。
fn match_skip_group(rest: &[u8]) -> Option<usize> {
    SKIP_GROUPS
        .iter()
        .find(|p| rest.starts_with(p))
        .map(|p| p.len())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "rtf_tests.rs"]
mod tests;
