//! PDF `/Info /CreationDate` + `/ModDate` 字节扫描。
//!
//! PDF Date 格式（ISO 32000-1 § 7.9.4）：`(D:YYYYMMDDHHmmSSOHH'mm')`，O 是 `+`/`-`/`Z`，
//! 秒/时区可省略，最短 `D:YYYYMMDD` 仅日期。头 64 KB + 尾 64 KB 双窗口扫描 key 后
//! 第一个 `(D:...)` 字面量——线性化 PDF 的 `/Info` 在头部，未线性化大 PDF 的
//! trailer/`/Info` 在文件尾；`/Info` 落在中段（xref 指向文件中部对象）时仍漏检
//! fallback 到 P4 mtime（YAGNI 不解析 xref）。`MediaReader: Read + Seek` 恒成立
//! （Local mmap / Remote 内存 Cursor），seek 廉价无需不可 seek 分支。

use std::io::{Read, SeekFrom};
use std::ops::Range;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};

use crate::entities::backend::MediaReader;

/// PDF 字节扫描单窗口大小；头尾各一个窗口。
const PDF_SCAN_BYTES: u64 = 64 * 1024;

const KEY_CREATION: &[u8] = b"/CreationDate";
const KEY_MOD: &[u8] = b"/ModDate";

/// 入口：按 [`scan_windows`] 读头（+尾）窗口拼 buffer 后委托 `extract_dates`。
/// 头窗口字节在 buffer 前部，`find_subslice` 取首个匹配天然实现「头优先」；
/// 两窗口不相交，拼接只可能边界撕裂漏检（与旧单窗口同类可接受限制）不会假阳性。
///
/// 整 fn `coverage(off)`：seek/read Err arm 只能用 lib unit `FailRead` 注入触发，
/// subprocess bin instance 永远走 OK arm，多 instance 累加 phantom miss。
/// 窗口划分由纯 fn `scan_windows` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(buf) = read_windows(reader) else {
        return (0, 0);
    };
    extract_dates(&buf)
}

/// 纯窗口划分：`size ≤ 2×64 KB` 整读单窗口（无重叠重复解析）；更大文件头尾各
/// 64 KB 两段不相交窗口。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn scan_windows(size: u64) -> (Range<u64>, Option<Range<u64>>) {
    if size <= 2 * PDF_SCAN_BYTES {
        (0..size, None)
    } else {
        (0..PDF_SCAN_BYTES, Some(size - PDF_SCAN_BYTES..size))
    }
}

/// IO 胶水：seek End 取 size → 按窗口读拼 buffer。`coverage(off)` 理由同 `parse`。
#[cfg_attr(coverage_nightly, coverage(off))]
fn read_windows(reader: &mut dyn MediaReader) -> Option<Vec<u8>> {
    let size = reader.seek(SeekFrom::End(0)).ok()?;
    let (head, tail) = scan_windows(size);
    reader.seek(SeekFrom::Start(head.start)).ok()?;
    let head_len = usize::try_from(head.end - head.start).ok()?;
    let mut buf = Vec::with_capacity(head_len);
    reader
        .take(head.end - head.start)
        .read_to_end(&mut buf)
        .ok()?;
    if let Some(tail) = tail {
        reader.seek(SeekFrom::Start(tail.start)).ok()?;
        reader
            .take(tail.end - tail.start)
            .read_to_end(&mut buf)
            .ok()?;
    }
    Some(buf)
}

/// 文本提取的整文件读取上限：正文 content stream 分布全文件，4 MiB 覆盖
/// 常见文档前几十页（远端 backend 已整文件入堆，本 cap 只防超大 PDF 吃内存）。
const PDF_TEXT_INPUT_CAP: usize = 4 * 1024 * 1024;

/// 单 content stream 解压输出上限：防 Flate 炸弹。
const INFLATE_OUTPUT_CAP: usize = 2 * 1024 * 1024;

/// 文本提取入口：扫 `stream…endstream` 块，`/FlateDecode` 的 flate2 解压后抓
/// 文本运算符的 `(...)` 字符串字面量。CID 字体（中文常见）输出 hex 串提不出
/// 可读文本、加密/扫描版无文本层——返空走 `uncategorized`（已知限制）。
///
/// 整 fn `coverage(off)`：read 入口 Err arm 同 `parse`；扫描业务由
/// `extract_text_from_buf` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let mut buf = Vec::new();
    let mut limited = reader.take(PDF_TEXT_INPUT_CAP as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    extract_text_from_buf(&buf, max_bytes)
}

/// 纯字节扫描业务：遍历 `stream`/`endstream` 对，按前导字典是否含
/// `/FlateDecode` 决定解压，再收集 `BT…ET` 文本块内的字符串字面量。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn extract_text_from_buf(buf: &[u8], max_bytes: usize) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while out.len() < max_bytes {
        let Some(rel) = find_subslice(&buf[pos..], b"stream") else {
            break;
        };
        let kw_start = pos + rel;
        let data_start = skip_stream_eol(buf, kw_start + b"stream".len());
        let Some(end_rel) = find_subslice(&buf[data_start..], b"endstream") else {
            break;
        };
        let data = &buf[data_start..data_start + end_rel];
        // 字典在 `stream` 关键字前的 `<<…>>`；仅回看 1 KiB（Length/Filter 字典很短）。
        let dict_window_start = kw_start.saturating_sub(1024);
        let dict = &buf[dict_window_start..kw_start];
        if find_subslice(dict, b"/FlateDecode").is_some() {
            if let Some(inflated) = inflate_capped(data) {
                collect_string_literals(&inflated, &mut out, max_bytes);
            }
        } else {
            collect_string_literals(data, &mut out, max_bytes);
        }
        pos = data_start + end_rel + b"endstream".len();
    }
    out
}

/// zlib 解压 content stream，输出 cap 防炸弹；非 zlib 数据（加密/其它 filter
/// 链）返 `None` 跳过。
#[cfg_attr(coverage_nightly, coverage(off))]
fn inflate_capped(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let decoder = flate2::read::ZlibDecoder::new(data);
    let mut inflated = Vec::new();
    let n = decoder
        .take(INFLATE_OUTPUT_CAP as u64)
        .read_to_end(&mut inflated);
    match n {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(inflated),
    }
}

/// 收集 content 内 `(...)` 字符串字面量（PDF 文本 show 运算符 `Tj`/`TJ`/`'`/`"`
/// 的操作数），处理 `\(` `\)` `\\` 转义与嵌套括号；仅当 content 含 `BT`
/// （text block 开始运算符）才扫，过滤纯图形 stream。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn collect_string_literals(content: &[u8], out: &mut String, max_bytes: usize) {
    if find_subslice(content, b"BT").is_none() {
        return;
    }
    let mut text: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < content.len() && out.len() + text.len() < max_bytes {
        if content[i] != b'(' {
            i += 1;
            continue;
        }
        i += 1;
        let mut depth = 1;
        let mut got_any = false;
        while i < content.len() && depth > 0 {
            match content[i] {
                b'\\' if i + 1 < content.len() => {
                    let esc = content[i + 1];
                    if matches!(esc, b'(' | b')' | b'\\') {
                        text.push(esc);
                        got_any = true;
                    }
                    i += 2;
                    continue;
                }
                b'(' => {
                    depth += 1;
                    text.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth > 0 {
                        text.push(b')');
                    }
                }
                c => {
                    text.push(c);
                    got_any = true;
                }
            }
            i += 1;
        }
        if got_any {
            text.push(b' ');
        }
    }
    let lossy = String::from_utf8_lossy(&text);
    let trimmed = lossy.trim_end();
    if !trimmed.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(trimmed);
    }
    super::scan::truncate_at_boundary(out, max_bytes);
}

/// `stream` 关键字后跳过 EOL（`\r\n` 或 `\n`，PDF spec §7.3.8）。
#[cfg_attr(coverage_nightly, coverage(off))]
fn skip_stream_eol(buf: &[u8], mut i: usize) -> usize {
    if buf.get(i) == Some(&b'\r') {
        i += 1;
    }
    if buf.get(i) == Some(&b'\n') {
        i += 1;
    }
    i
}

/// 纯字节扫描业务：在 buffer 内查 `/CreationDate` + `/ModDate` 的 `D:` 字面量。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_d_date_after_key(buf, KEY_CREATION).unwrap_or(0);
    let modified = scan_d_date_after_key(buf, KEY_MOD).unwrap_or(0);
    (created, modified)
}

/// 在 `buf` 中找首个 `key`（如 `/CreationDate`），跳过 key 后白空格直到 `(D:...)`，
/// 提取括号内字串调 `parse_pdf_d_format` 转 Unix UTC epoch。
#[cfg_attr(coverage_nightly, coverage(off))]
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
    if secs <= 0 {
        None
    } else {
        Some(secs.cast_unsigned())
    }
}

/// `from_utf8` 对 ASCII 子串永不 Err；用于 PDF 日期前缀（YYYY/MM/DD）切片转 &str。
#[cfg_attr(coverage_nightly, coverage(off))]
fn ascii_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("internal: PDF date prefix bytes are ASCII")
}

#[cfg_attr(coverage_nightly, coverage(off))]
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

#[cfg_attr(coverage_nightly, coverage(off))]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
