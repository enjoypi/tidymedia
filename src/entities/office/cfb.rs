use std::io::Read;

use crate::entities::backend::MediaReader;

use super::cfb_io;

const SUMMARY_INFORMATION_STREAM: &str = "/\x05SummaryInformation";
const PID_CREATE_DTM: u32 = 0x0C;
const PID_LASTSAVE_DTM: u32 = 0x0D;
const VT_FILETIME: u32 = 0x40;
const FORMAT_ID_SUMMARY: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
const FILETIME_TICKS_PER_SEC: u64 = 10_000_000;
const EPOCH_DELTA_SECS: u64 = 11_644_473_600;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(mut comp) = cfb_io::open_cfb(reader) else {
        return (0, 0);
    };
    let Some(stream) = cfb_io::open_stream(&mut comp, SUMMARY_INFORMATION_STREAM) else {
        return (0, 0);
    };
    let Some(bytes) = cfb_io::read_stream(stream) else {
        return (0, 0);
    };
    extract_dates(&bytes)
}

const STREAM_TEXT_INPUT_CAP: u64 = 1024 * 1024;
const MIN_RUN_CHARS: usize = 8;

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(mut comp) = cfb_io::open_cfb(reader) else {
        return String::new();
    };
    let paths = cfb_io::stream_paths(&comp, |name| !name.starts_with('\x05'));
    let mut out = String::new();
    for p in paths {
        if out.len() >= max_bytes {
            break;
        }
        let Some(stream) = cfb_io::open_stream(&mut comp, &p) else {
            continue;
        };
        let Some(bytes) = cfb_io::read_stream(stream.take(STREAM_TEXT_INPUT_CAP)) else {
            continue;
        };
        extract_printable_runs(&bytes, &mut out, max_bytes);
    }
    out
}

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

#[doc(hidden)]
#[must_use]
pub fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = find_property_filetime(buf, PID_CREATE_DTM).unwrap_or(0);
    let modified = find_property_filetime(buf, PID_LASTSAVE_DTM).unwrap_or(0);
    (created, modified)
}

#[doc(hidden)]
#[must_use]
pub fn find_property_filetime(buf: &[u8], pid: u32) -> Option<u64> {
    if buf.len() < 48 {
        return None;
    }
    let byte_order = u16::from_le_bytes([buf[0], buf[1]]);
    if byte_order != 0xFFFE {
        return None;
    }
    if buf[28..44] != FORMAT_ID_SUMMARY {
        return None;
    }
    let section_off = u32_le_at(buf, 44) as usize;
    let section = buf.get(section_off..)?;
    if section.len() < 8 {
        return None;
    }
    let num_props = u32_le_at(section, 4) as usize;
    if num_props > 256 {
        return None;
    }
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
    let ptype = u32_le_at(prop, 0);
    if ptype != VT_FILETIME {
        return None;
    }
    let mut ticks_bytes = [0u8; 8];
    ticks_bytes.copy_from_slice(&prop[4..12]);
    filetime_to_epoch(u64::from_le_bytes(ticks_bytes))
}

#[doc(hidden)]
#[must_use]
pub fn filetime_to_epoch(ticks: u64) -> Option<u64> {
    let secs = ticks / FILETIME_TICKS_PER_SEC;
    if secs <= EPOCH_DELTA_SECS {
        return None;
    }
    Some(secs - EPOCH_DELTA_SECS)
}

#[doc(hidden)]
#[must_use]
pub fn u32_le_at(buf: &[u8], off: usize) -> u32 {
    let mut arr = [0u8; 4];
    arr.copy_from_slice(&buf[off..off + 4]);
    u32::from_le_bytes(arr)
}

#[cfg(test)]
#[path = "cfb_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "cfb_tests.rs"]
mod tests;
