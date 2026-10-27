//! PDF 日期解析族：扫 `/CreationDate` / `/ModDate` key 后首个 `(D:...)` 字面量，
//! 按 PDF Date 格式（ISO 32000-1 § 7.9.4）解析为 Unix UTC epoch。
//! 入口 `extract_dates` 被父模块 `parse` 调用；共享 `find_subslice` 在父模块。

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};

use super::find_subslice;

const KEY_CREATION: &[u8] = b"/CreationDate";
const KEY_MOD: &[u8] = b"/ModDate";

/// 纯字节扫描业务：在 buffer 内查 `/CreationDate` + `/ModDate` 的 `D:` 字面量。
pub fn extract_dates(buf: &[u8], default_offset: FixedOffset) -> (u64, u64) {
    let created = scan_d_date_after_key(buf, KEY_CREATION, default_offset).unwrap_or(0);
    let modified = scan_d_date_after_key(buf, KEY_MOD, default_offset).unwrap_or(0);
    (created, modified)
}

/// 在 `buf` 中找首个 `key`（如 `/CreationDate`），跳过 key 后白空格直到 `(D:...)`，
/// 提取括号内字串调 `parse_pdf_d_format` 转 Unix UTC epoch。
pub(super) fn scan_d_date_after_key(
    buf: &[u8],
    key: &[u8],
    default_offset: FixedOffset,
) -> Option<u64> {
    let pos = find_subslice(buf, key)?;
    let rest = &buf[pos + key.len()..];
    let open = find_byte(rest, b'(')?;
    let payload = &rest[open + 1..];
    let close = find_byte(payload, b')')?;
    let s = std::str::from_utf8(&payload[..close]).ok()?;
    parse_pdf_d_format(s, default_offset)
}

/// `slice::iter().position(closure)` 的非 closure 等价物 —— closure region 在多
/// codegen instance 下易出 phantom miss（tests/CLAUDE.md「覆盖率处理」）；
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

pub fn parse_pdf_d_format(s: &str, default_offset: FixedOffset) -> Option<u64> {
    let body = s.strip_prefix("D:")?;
    let bytes = body.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    let year: i32 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month: u32 = parse_pair(bytes, 4)?;
    let day: u32 = parse_pair(bytes, 6)?;
    let hour = parse_pair(bytes, 8).unwrap_or(0);
    let minute = parse_pair(bytes, 10).unwrap_or(0);
    let second = parse_pair(bytes, 12).unwrap_or(0);
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let naive = date.and_hms_opt(hour, minute, second)?;
    let offset = parse_tz_offset(&bytes[14.min(bytes.len())..], default_offset);
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

pub(super) fn parse_pair(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 2)?;
    std::str::from_utf8(slice).ok()?.parse().ok()
}

pub(super) fn parse_tz_offset(tz: &[u8], default_offset: FixedOffset) -> FixedOffset {
    match tz.first() {
        Some(b'+') => signed_offset(1, tz).unwrap_or(default_offset),
        Some(b'-') => signed_offset(-1, tz).unwrap_or(default_offset),
        _ => utc_offset(),
    }
}

fn signed_offset(sign: i32, tz: &[u8]) -> Option<FixedOffset> {
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
    if mm >= 60 {
        return None;
    }
    FixedOffset::east_opt(sign * (hh * 3600 + mm * 60))
}

fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).expect("internal: zero offset is within range")
}
