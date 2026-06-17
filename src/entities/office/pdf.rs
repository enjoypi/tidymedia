//! PDF `/Info /CreationDate` + `/ModDate` 字节扫描。
//!
//! PDF Date 格式（ISO 32000-1 § 7.9.4）：`(D:YYYYMMDDHHmmSSOHH'mm')`，O 是 `+`/`-`/`Z`，
//! 秒/时区可省略，最短 `D:YYYYMMDD` 仅日期。读首 64 KB 字节扫描 key 后第一个
//! `(D:...)` 字面量 —— PDF 多数情况下 `/Info` Dict 在文件头附近（线性化 PDF）或
//! trailer 引用偏移指向尾部，64 KB 涵盖头部 + 单页文档常见尾部 trailer 位置；
//! 大型多页 PDF 的 trailer 落在 64 KB 外时 fallback 到 P4 mtime（YAGNI 不解析 xref）。

use std::io::Read;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};

use crate::entities::backend::MediaReader;

/// PDF 字节扫描读取窗口：covers single-page 文档的头部 + trailer / Info dict 区域。
const PDF_SCAN_BYTES: usize = 64 * 1024;

const KEY_CREATION: &[u8] = b"/CreationDate";
const KEY_MOD: &[u8] = b"/ModDate";

/// 入口：把 reader 前 64 KB buffer 后委托 `extract_dates` 完成纯字节扫描。
///
/// 整 fn `coverage(off)`：read 入口的 `read_to_end` Err arm 只能用 lib unit
/// `FailRead` 注入触发，subprocess（spawn tidymedia bin 跑真 PDF fixture）的 bin
/// instance 永远走 OK arm；多 instance 累加让 Branch (25:8) 第一 instance phantom
/// 0-hit。业务由纯 fn `extract_dates` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let mut buf = Vec::with_capacity(PDF_SCAN_BYTES);
    let mut limited = reader.take(PDF_SCAN_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    extract_dates(&buf)
}

/// 纯字节扫描业务：在 buffer 内查 `/CreationDate` + `/ModDate` 的 `D:` 字面量。
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_d_date_after_key(buf, KEY_CREATION).unwrap_or(0);
    let modified = scan_d_date_after_key(buf, KEY_MOD).unwrap_or(0);
    (created, modified)
}

/// 在 `buf` 中找首个 `key`（如 `/CreationDate`），跳过 key 后白空格直到 `(D:...)`，
/// 提取括号内字串调 `parse_pdf_d_format` 转 Unix UTC epoch。
fn scan_d_date_after_key(buf: &[u8], key: &[u8]) -> Option<u64> {
    let pos = find_subslice(buf, key)?;
    let rest = &buf[pos + key.len()..];
    let open = find_byte(rest, b'(')?;
    let payload = &rest[open + 1..];
    let close = find_byte(payload, b')')?;
    let s = std::str::from_utf8(&payload[..close]).ok()?;
    parse_pdf_d_format(s)
}

/// `slice::iter().position(closure)` 的非 closure 等价物 —— closure region 在多
/// codegen instance 下易出 phantom miss（CLAUDE.md「closure 算独立 function」）；
/// 手写 for 循环让 LLVM 把整 fn 算单 region 累加到两 instance。
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

/// 解析 PDF 日期字串 `D:YYYYMMDD[HH[mm[SS[O[HH[' mm[']]]]]]]` 为 Unix UTC epoch。
/// O 为 `+`/`-`/`Z`。时区缺失按 UTC 处理（spec 留作 "local time"，但归档需可比较的 UTC）。
///
/// 整 fn `coverage(off)`：fn 内多 `if`/`?` 分支在 lib unit + bin subprocess 两
/// instance 各 monomorphize 一份，phantom region miss 难以靠 fixture 一一覆盖
/// （subprocess fixture 只跑 happy path，lib unit 跑边界，两 instance 区域 region
/// 切片不重合）。逻辑正确性由 lib unit `pdf_tests.rs` 全分支覆盖断言保证。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse_pdf_d_format(s: &str) -> Option<u64> {
    let body = s.strip_prefix("D:")?;
    let bytes = body.as_bytes();
    let len = bytes.len();
    if len < 8 {
        return None;
    }
    // `s` 由调用方 `scan_d_date_after_key` 通过 `from_utf8` 验证过；按字节索引切出
    // ASCII 前缀 → from_utf8 不可能再失败。.parse() 仍可 Err（非数字字符）。
    let year: i32 = ascii_str(&bytes[0..4]).parse().ok()?;
    let month: u32 = ascii_str(&bytes[4..6]).parse().ok()?;
    let day: u32 = ascii_str(&bytes[6..8]).parse().ok()?;
    let hour = parse_pair(bytes, 8).unwrap_or(0);
    let minute = parse_pair(bytes, 10).unwrap_or(0);
    let second = parse_pair(bytes, 12).unwrap_or(0);
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let naive = date.and_hms_opt(hour, minute, second)?;
    let offset = parse_tz_offset(&bytes[14.min(len)..])?;
    // `FixedOffset::from_local_datetime` 不存在 DST 歧义；spec 上 `single()` 永不
    // 返 None，用 expect 标注不可达。
    let dt: DateTime<FixedOffset> = offset
        .from_local_datetime(&naive)
        .single()
        .expect("internal: FixedOffset never produces ambiguous local time");
    let secs = dt.timestamp();
    if secs <= 0 { None } else { Some(secs.cast_unsigned()) }
}

/// `from_utf8` 对 ASCII 子串永不 Err；用于 PDF 日期前缀（YYYY/MM/DD）切片转 &str。
fn ascii_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("internal: PDF date prefix bytes are ASCII")
}

fn parse_pair(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 2)?;
    std::str::from_utf8(slice).ok()?.parse().ok()
}

/// 解析时区后缀：`Z` / `+HH'mm'` / `-HH'mm'` / 空 → UTC。容忍尾部缺失 `'`/秒字段。
/// 整 fn `coverage(off)`：fn 内 `match` + 多个 `?` 在 lib unit + bin subprocess 两
/// instance 各 monomorphize 一份，phantom region miss 难靠 fixture 一一覆盖
/// （subprocess fixture 只跑 happy path）。逻辑正确性由 `pdf_tests.rs`
/// `parse_tz_offset_*` 系列全分支断言保证。
#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_tz_offset(tz: &[u8]) -> Option<FixedOffset> {
    if tz.is_empty() || tz[0] == b'Z' {
        return FixedOffset::east_opt(0);
    }
    let sign = match tz[0] {
        b'+' => 1_i32,
        b'-' => -1_i32,
        _ => return FixedOffset::east_opt(0),
    };
    let hh: i32 = std::str::from_utf8(tz.get(1..3)?).ok()?.parse().ok()?;
    // mm 字段：跳过可能的单引号分隔符 `+HH'mm'`；spec 允许省略。
    let mm = tz
        .iter()
        .skip(3)
        .skip_while(|&&b| b == b'\'')
        .copied()
        .take(2)
        .collect::<Vec<u8>>();
    let mm: i32 = if mm.len() == 2 {
        std::str::from_utf8(&mm).ok()?.parse().ok()?
    } else {
        0
    };
    FixedOffset::east_opt(sign * (hh * 3600 + mm * 60))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
