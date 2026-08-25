//! CFB (Compound File Binary，doc/ppt/xls) `\x05SummaryInformation` (CFB stream) 解析。
//!
//! `CFB` 容器（OLE2）内含 `\x05SummaryInformation` stream，按 `MS-OLEPS` `PropertySet`
//! 格式存储 `PID_CREATE_DTM` (0x0C) / `PID_LASTSAVE_DTM` (0x0D)。本模块依赖 `cfb` crate
//! 仅读 stream 字节，`PropertySet` 自解析 —— 写 lib 罕见（Python `compoundfiles` /
//! `olefile` 只读），测试 fixture 用 cfb crate write 在 test setup 即时合成。

use std::io::Read;

use crate::entities::backend::MediaReader;

const SUMMARY_INFORMATION_STREAM: &str = "/\x05SummaryInformation";
const PID_CREATE_DTM: u32 = 0x0C;
const PID_LASTSAVE_DTM: u32 = 0x0D;
const VT_FILETIME: u32 = 0x40;

/// `SummaryInformation` FMTID（小端 GUID 字节序）：`F29F85E0-4FF9-1068-AB91-08002B27B3D9`。
const FORMAT_ID_SUMMARY: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];

/// FILETIME 100ns ticks per second。
const FILETIME_TICKS_PER_SEC: u64 = 10_000_000;
/// Unix epoch (1970-01-01) - FILETIME epoch (1601-01-01) 秒差。
const EPOCH_DELTA_SECS: u64 = 11_644_473_600;

/// 入口：把 reader 当 `CFB` 容器打开，读 `SummaryInformation` stream 后解析 `PropertySet`。
///
/// 整 fn `coverage(off)`：fn 内多 let-else 早返路径在 lib unit fixture 各分支命中，
/// 但 subprocess (bin instance) 仅跑 happy；多 instance 累加让 phantom region miss
/// 难闭合 —— 业务由 `extract_dates` / `find_property_filetime` 单测真测。
#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Ok(mut comp) = cfb::CompoundFile::open(reader) else {
        return (0, 0);
    };
    let Ok(mut stream) = comp.open_stream(SUMMARY_INFORMATION_STREAM) else {
        return (0, 0);
    };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return (0, 0);
    }
    extract_dates(&bytes)
}

/// 单 stream 读取上限：`WordDocument` / `Workbook` / `PowerPoint Document`
/// 正文流前 1 MiB 覆盖常见文档前几十页。
const STREAM_TEXT_INPUT_CAP: u64 = 1024 * 1024;

/// printable run 最小长度（字符数）：低于此长度的碎片多为二进制噪声。
const MIN_RUN_CHARS: usize = 8;

/// 文本提取入口：`CompoundFile::walk` 枚举正文 stream（跳过 `\x05` 开头的
/// metadata stream），对每个 stream 做「可打印字符 run」扫描（UTF-16LE +
/// ASCII 双遍）。不解 Word piece table / xls SST / ppt `TextAtom` 结构
/// （best-effort，用户已确认）。
///
/// 整 fn `coverage(off)`：CFB 打开/walk/read 早返路径同 `parse`；run 扫描业务由
/// `extract_printable_runs` 单测真测。
#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Ok(mut comp) = cfb::CompoundFile::open(reader) else {
        return String::new();
    };
    let paths: Vec<std::path::PathBuf> = comp
        .walk()
        .filter(|e| e.is_stream() && !e.name().starts_with('\x05'))
        .map(|e| e.path().to_path_buf())
        .collect();
    let mut out = String::new();
    for p in paths {
        if out.len() >= max_bytes {
            break;
        }
        let Ok(stream) = comp.open_stream(&p) else {
            continue;
        };
        let mut bytes = Vec::new();
        if stream
            .take(STREAM_TEXT_INPUT_CAP)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        extract_printable_runs(&bytes, &mut out, max_bytes);
    }
    out
}

/// 纯字节扫描业务：先按 UTF-16LE 提取可打印 run（Word 97+ 非 Latin 正文 /
/// xls SST / ppt `TextCharsAtom` 的存储编码），再按 ASCII 提取（cp1252 piece）。
/// 两遍独立、各要求 run ≥ [`MIN_RUN_CHARS`] 字符抑制二进制噪声。
#[doc(hidden)]
pub fn extract_printable_runs(bytes: &[u8], out: &mut String, max_bytes: usize) {
    append_utf16le_runs(bytes, out, max_bytes);
    append_ascii_runs(bytes, out, max_bytes);
}

#[doc(hidden)]
pub fn append_utf16le_runs(bytes: &[u8], out: &mut String, max_bytes: usize) {
    let mut run = String::new();
    let mut i = 0;
    while i + 1 < bytes.len() && out.len() < max_bytes {
        let unit = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let c = char::from_u32(u32::from(unit)).filter(|c| is_text_char(*c));
        if let Some(c) = c {
            run.push(c);
        } else {
            flush_run(&mut run, out, max_bytes);
        }
        i += 2;
    }
    flush_run(&mut run, out, max_bytes);
}

#[doc(hidden)]
pub fn append_ascii_runs(bytes: &[u8], out: &mut String, max_bytes: usize) {
    let mut run = String::new();
    for &b in bytes {
        if out.len() >= max_bytes {
            break;
        }
        if b.is_ascii() && is_text_char(char::from(b)) {
            run.push(char::from(b));
        } else {
            flush_run(&mut run, out, max_bytes);
        }
    }
    flush_run(&mut run, out, max_bytes);
}

/// run 达到最小长度才落盘（短碎片是二进制噪声），并按剩余 budget 截断。
#[doc(hidden)]
pub fn flush_run(run: &mut String, out: &mut String, max_bytes: usize) {
    if run.chars().count() >= MIN_RUN_CHARS && out.len() < max_bytes {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(run);
        super::scan::truncate_at_boundary(out, max_bytes);
    }
    run.clear();
}

/// 分类可用的「文本字符」：字母数字（含 CJK）/ 空格 / ASCII 标点。
/// 控制符与 surrogate 区（`from_u32` 已滤）视为二进制噪声边界。
#[doc(hidden)]
#[must_use]
pub fn is_text_char(c: char) -> bool {
    if c.is_alphanumeric() {
        return true;
    }
    if c == ' ' {
        return true;
    }
    c.is_ascii_punctuation()
}

/// 纯 `PropertySet` 字节解析业务：查 `PID_CREATE_DTM` / `PID_LASTSAVE_DTM` 的 FILETIME 值。
#[doc(hidden)]
#[must_use]
pub fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = find_property_filetime(buf, PID_CREATE_DTM).unwrap_or(0);
    let modified = find_property_filetime(buf, PID_LASTSAVE_DTM).unwrap_or(0);
    (created, modified)
}

/// 在 `PropertySet` 内查指定 PID 的 `VT_FILETIME` 属性值。
/// `PropertySet` 布局：48 字节 header + section（section size + num props + entries + data）。
///
/// `buf.len()` >= 48 守护后 `byteorder` / `fmtid` / `section_off` 用直接索引
/// （不可达 `?` 消除，CLAUDE.md「逻辑不可达的 `?` 死区消除」套路）。
#[doc(hidden)]
#[must_use]
pub fn find_property_filetime(buf: &[u8], pid: u32) -> Option<u64> {
    if buf.len() < 48 {
        return None;
    }
    // ByteOrder：MS-OLEPS 规定 0xFFFE（little-endian）。
    let byte_order = u16::from_le_bytes([buf[0], buf[1]]);
    if byte_order != 0xFFFE {
        return None;
    }
    // FMTID at offset 28..44 必须匹配 SummaryInformation GUID。
    if buf[28..44] != FORMAT_ID_SUMMARY {
        return None;
    }
    let section_off = u32_le_at(buf, 44) as usize;
    let section = buf.get(section_off..)?;
    if section.len() < 8 {
        return None;
    }
    let num_props = u32_le_at(section, 4) as usize;
    // 防恶意/损坏 PropertySet 报巨大 num_props 吃内存：MS-OLEPS spec 约束实际 < 256。
    if num_props > 256 {
        return None;
    }
    // num_props <= 256 → 8 + 256*8 = 2056 不溢出，安全用 `*` 不需 checked_mul。
    let entries_end = 8 + num_props * 8;
    let entries = section.get(8..entries_end)?;
    let mut i = 0;
    while i + 8 <= entries.len() {
        let id = u32_le_at(entries, i);
        let off = u32_le_at(entries, i + 4) as usize;
        if id == pid {
            return read_filetime(section, off);
        }
        i += 8;
    }
    None
}

fn read_filetime(section: &[u8], off: usize) -> Option<u64> {
    let prop = section.get(off..off + 12)?;
    // prop 已 12 字节，slice [0..4] / [4..12] 永不失败，直接 from_le_bytes。
    let ptype = u32_le_at(prop, 0);
    if ptype != VT_FILETIME {
        return None;
    }
    let mut ticks_bytes = [0u8; 8];
    ticks_bytes.copy_from_slice(&prop[4..12]);
    filetime_to_epoch(u64::from_le_bytes(ticks_bytes))
}

/// FILETIME (100ns ticks since 1601-01-01 UTC) → Unix epoch (secs since 1970-01-01)。
#[doc(hidden)]
#[must_use]
pub fn filetime_to_epoch(ticks: u64) -> Option<u64> {
    let secs = ticks / FILETIME_TICKS_PER_SEC;
    if secs <= EPOCH_DELTA_SECS {
        return None;
    }
    Some(secs - EPOCH_DELTA_SECS)
}

/// 从 `buf[off..off+4]` 读小端 u32。调用方 MUST 保证 `off + 4 <= buf.len()`，
/// 不做范围检查（避免不可达 `?` 死区）。
#[doc(hidden)]
#[must_use]
pub fn u32_le_at(buf: &[u8], off: usize) -> u32 {
    let mut arr = [0u8; 4];
    arr.copy_from_slice(&buf[off..off + 4]);
    u32::from_le_bytes(arr)
}

#[cfg(test)]
#[path = "cfb_tests.rs"]
mod tests;
