//! PDF 日期解析族：扫 `/CreationDate` / `/ModDate` key 后首个 `(D:...)` 字面量，
//! 按 PDF Date 格式（ISO 32000-1 § 7.9.4）解析为 Unix UTC epoch。
//! 入口 `extract_dates` 被父模块 `parse` 调用；共享 `find_subslice` 在父模块。

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};

use super::find_subslice;

const KEY_CREATION: &[u8] = b"/CreationDate";
const KEY_MOD: &[u8] = b"/ModDate";

/// 纯字节扫描业务：在 buffer 内查 `/CreationDate` + `/ModDate` 的 `D:` 字面量。
pub fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_d_date_after_key(buf, KEY_CREATION).unwrap_or(0);
    let modified = scan_d_date_after_key(buf, KEY_MOD).unwrap_or(0);
    (created, modified)
}

/// 在 `buf` 中找首个 `key`（如 `/CreationDate`），跳过 key 后白空格直到 `(D:...)`，
/// 提取括号内字串调 `parse_pdf_d_format` 转 Unix UTC epoch。
pub(super) fn scan_d_date_after_key(buf: &[u8], key: &[u8]) -> Option<u64> {
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
pub fn parse_pdf_d_format(s: &str) -> Option<u64> {
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
    if secs <= 0 {
        None
    } else {
        Some(secs.cast_unsigned())
    }
}

/// `from_utf8` 对 ASCII 子串永不 Err；用于 PDF 日期前缀（YYYY/MM/DD）切片转 &str。
fn ascii_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("internal: PDF date prefix bytes are ASCII")
}

pub(super) fn parse_pair(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 2)?;
    std::str::from_utf8(slice).ok()?.parse().ok()
}

/// 解析时区后缀：`Z` / `+HH'mm'` / `-HH'mm'` / 空 → UTC。容忍尾部缺失 `'`/秒字段。
/// 整 fn `coverage(off)`：fn 内 `match` + 多个 `?` 在 lib unit + bin subprocess 两
/// instance 各 monomorphize 一份，phantom region miss 难靠 fixture 一一覆盖
/// （subprocess fixture 只跑 happy path）。逻辑正确性由 `pdf_tests.rs`
/// `parse_tz_offset_*` 系列全分支断言保证。
pub(super) fn parse_tz_offset(tz: &[u8]) -> Option<FixedOffset> {
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
