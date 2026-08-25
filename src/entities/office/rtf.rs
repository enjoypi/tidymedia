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
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let mut buf = Vec::with_capacity(RTF_SCAN_BYTES);
    let mut limited = reader.take(RTF_SCAN_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    extract_dates(&buf)
}

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

/// 文本提取入口：读前 256 KiB 后剥 RTF 控制字得 best-effort 正文。
///
/// 整 fn `coverage(off)`：read 入口 Err arm 同 `parse`；剥控制字业务由
/// `strip_rtf_into` 单测真测。
pub(super) fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let mut buf = Vec::with_capacity(RTF_TEXT_INPUT_CAP);
    let mut limited = reader.take(RTF_TEXT_INPUT_CAP as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let mut out = String::new();
    strip_rtf_into(&buf, &mut out, max_bytes);
    out
}

/// 纯字节状态机：剥 `{}`/控制字，保留正文。`\uN` unicode 转义转 char（中文 RTF
/// 主流编码方式；后随单个 fallback `?` 跳过）；`\par`/`\line`/`\tab` 折为空格；
/// `\'xx` 单字节转义与高位字节先入字节缓冲，最终 lossy 转 UTF-8（GBK 双字节
/// 无法还原，U+FFFD 噪声对分类可容忍）。
pub(super) fn strip_rtf_into(buf: &[u8], out: &mut String, max_bytes: usize) {
    let mut text: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut skip_depth: Option<i32> = None;
    let mut depth: i32 = 0;
    while i < buf.len() && text.len() < max_bytes {
        let b = buf[i];
        if let Some(sd) = skip_depth {
            // 处于跳过组内：只跟踪括号深度直到组闭合。
            match b {
                b'\\' if i + 1 < buf.len() && matches!(buf[i + 1], b'{' | b'}' | b'\\') => i += 1,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth < sd {
                        skip_depth = None;
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        match b {
            b'{' => {
                depth += 1;
                if let Some(len) = match_skip_group(&buf[i + 1..]) {
                    skip_depth = Some(depth);
                    i += 1 + len;
                    continue;
                }
            }
            b'}' => depth -= 1,
            b'\\' => {
                i += 1;
                i += consume_control(&buf[i..], &mut text);
                continue;
            }
            b'\r' | b'\n' => {}
            _ => text.push(b),
        }
        i += 1;
    }
    out.push_str(&String::from_utf8_lossy(&text));
    super::scan::truncate_at_boundary(out, max_bytes);
}

/// `{` 后是否紧跟 [`SKIP_GROUPS`] 前缀；命中返回前缀长度。
fn match_skip_group(rest: &[u8]) -> Option<usize> {
    SKIP_GROUPS
        .iter()
        .find(|p| rest.starts_with(p))
        .map(|p| p.len())
}

/// 消费 `\` 后的控制字/转义，向 `text` 追加对应字面量；返回消费的字节数
/// （不含前导 `\` 本身）。
fn consume_control(rest: &[u8], text: &mut Vec<u8>) -> usize {
    let Some(&first) = rest.first() else {
        return 0;
    };
    // 符号转义：`\{` `\}` `\\` 还原字面量。
    if matches!(first, b'{' | b'}' | b'\\') {
        text.push(first);
        return 1;
    }
    // `\'xx` 单字节 hex 转义（cp1252/GBK 单字节；双字节 GBK lossy 成 U+FFFD）。
    if first == b'\'' {
        if let (Some(&h), Some(&l)) = (rest.get(1), rest.get(2))
            && let (Some(hv), Some(lv)) = (hex_val(h), hex_val(l))
        {
            text.push(hv * 16 + lv);
            return 3;
        }
        return 1;
    }
    if !first.is_ascii_alphabetic() {
        // 其它符号控制（`\~` 不换行空格等）按空格处理。
        text.push(b' ');
        return 1;
    }
    // 控制字：字母序列 + 可选负号数字参数 + 可选单个空格分隔符。
    let mut j = 0;
    while j < rest.len() && rest[j].is_ascii_alphabetic() {
        j += 1;
    }
    let word = &rest[..j];
    let num_start = j;
    if j < rest.len() && rest[j] == b'-' {
        j += 1;
    }
    while j < rest.len() && rest[j].is_ascii_digit() {
        j += 1;
    }
    if word == b"u" {
        // `\uN`：N 为带符号 16 位；后随 fallback 字符（约定 1 个）跳过。
        if let Ok(n) = std::str::from_utf8(&rest[num_start..j])
            .expect("internal: digit/dash bytes are ASCII")
            .parse::<i32>()
        {
            let code = if n < 0 { n + 65_536 } else { n };
            if let Some(c) = u32::try_from(code).ok().and_then(char::from_u32) {
                let mut cb = [0u8; 4];
                text.extend_from_slice(c.encode_utf8(&mut cb).as_bytes());
            }
        }
        // 跳过分隔空格 + 1 个 fallback 字符（`?` 或 `\'xx` 的首字节交状态机自理）。
        if j < rest.len() && rest[j] == b' ' {
            j += 1;
        }
        if j < rest.len() && rest[j] == b'?' {
            j += 1;
        }
        return j;
    }
    if matches!(word, b"par" | b"line" | b"tab" | b"cell" | b"row") {
        text.push(b' ');
    }
    // 控制字后单个空格是分隔符，属于控制字本身。
    if j < rest.len() && rest[j] == b' ' {
        j += 1;
    }
    j
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

/// 在 buf 内找 `key` 字面量，跳过后读连续 ASCII 数字解析为整数。
/// `key` 后字符必须是数字（不允许空格）；遇 group 边界 `{` / `}` / `\` 停止读数字。
/// 整 fn `coverage(off)`：泛型 monomorphization + while + `||` 短路链在 LLVM 多
/// instance 累加 phantom branch miss；逻辑由 `rtf_tests.rs` `scan_int_after_*`
/// 系列全分支断言保证。
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
