//! RTF 解析入口与复杂业务 fn。
//!
//! 这些 fn 被 `OFFICE_FIXTURES` bin subprocess / `office_deep` 集成测试命中时只走
//! happy path，变体分支在对应实例计 0（per-instance phantom branch 缺口）。无法经
//! 测试补齐，按「函数独立 + ignore-regex 排除」处理（CLAUDE.md office 子模块套路）。

use std::io::Read;

use crate::entities::backend::MediaReader;
use crate::entities::office::scan::truncate_at_boundary;

use super::{
    RTF_SCAN_BYTES, RTF_TEXT_INPUT_CAP, extract_dates, find_subslice, hex_val, match_skip_group,
};

/// 入口：读 reader 前 64 KB 后字节扫描 `\creatim` / `\revtim` 控制字组。
#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let mut buf = Vec::with_capacity(RTF_SCAN_BYTES);
    let mut limited = reader.take(RTF_SCAN_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    extract_dates(&buf)
}

/// 文本提取入口：读前 256 KiB 后剥 RTF 控制字得 best-effort 正文。
#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
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
#[doc(hidden)]
pub fn strip_rtf_into(buf: &[u8], out: &mut String, max_bytes: usize) {
    let mut text: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut skip_depth: Option<i32> = None;
    let mut depth: i32 = 0;
    while i < buf.len() && text.len() < max_bytes {
        let b = buf[i];
        if let Some(sd) = skip_depth {
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
    truncate_at_boundary(out, max_bytes);
}

/// 消费 `\` 后的控制字/转义，向 `text` 追加对应字面量；返回消费的字节数
/// （不含前导 `\` 本身）。
#[doc(hidden)]
pub fn consume_control(rest: &[u8], text: &mut Vec<u8>) -> usize {
    let Some(&first) = rest.first() else {
        return 0;
    };
    if matches!(first, b'{' | b'}' | b'\\') {
        text.push(first);
        return 1;
    }
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
        text.push(b' ');
        return 1;
    }
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
    if j < rest.len() && rest[j] == b' ' {
        j += 1;
    }
    j
}

/// 在 buf 内找 `key` 字面量，跳过后读连续 ASCII 数字解析为整数。
/// `key` 后字符必须是数字（不允许空格）；遇 group 边界 `{` / `}` / `\` 停止读数字。
/// 泛型 monomorphization + while + `||` 短路链在 LLVM 多 instance 累加 phantom
/// branch miss；逻辑由 `rtf_tests.rs` `scan_int_after_*` 系列全分支断言保证。
#[doc(hidden)]
#[must_use]
pub fn scan_int_after<T: std::str::FromStr>(buf: &[u8], key: &[u8]) -> Option<T> {
    let pos = find_subslice(buf, key)?;
    let after = &buf[pos + key.len()..];
    let mut end = 0;
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
    std::str::from_utf8(&after[..end])
        .expect("internal: digit/dash bytes are ASCII")
        .parse()
        .ok()
}
