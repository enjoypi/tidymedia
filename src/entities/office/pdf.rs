use std::ops::Range;

use chrono::FixedOffset;

use crate::entities::backend::MediaReader;

use super::read_io;

#[path = "pdf_dates.rs"]
mod dates;
#[path = "pdf_text.rs"]
mod text;

pub(super) use dates::extract_dates;
#[cfg(test)]
pub(super) use dates::parse_pdf_d_format;
#[cfg(test)]
use dates::{parse_pair, parse_tz_offset, scan_d_date_after_key};
#[cfg(test)]
pub use text::collect_string_literals;
#[doc(hidden)]
pub use text::extract_text_from_buf;

const PDF_SCAN_BYTES: u64 = 64 * 1024;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str, default_offset: FixedOffset) -> (u64, u64) {
    let Some(buf) = read_windows(reader) else {
        return (0, 0);
    };
    extract_dates(&buf, default_offset)
}

#[doc(hidden)]
#[must_use]
pub fn scan_windows(size: u64) -> (Range<u64>, Option<Range<u64>>) {
    if size <= 2 * PDF_SCAN_BYTES {
        (0..size, None)
    } else {
        (0..PDF_SCAN_BYTES, Some(size - PDF_SCAN_BYTES..size))
    }
}

fn read_windows(reader: &mut dyn MediaReader) -> Option<Vec<u8>> {
    let size = read_io::seek_len(reader)?;
    let (head, tail) = scan_windows(size);
    let mut buf = read_io::read_range(reader, head)?;
    if let Some(tail) = tail {
        let mut tail_buf = read_io::read_range(reader, tail)?;
        buf.append(&mut tail_buf);
    }
    Some(buf)
}

const PDF_TEXT_INPUT_CAP: usize = 4 * 1024 * 1024;

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(buf) = read_io::read_capped(reader, PDF_TEXT_INPUT_CAP) else {
        return String::new();
    };
    extract_text_from_buf(&buf, max_bytes)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "pdf_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "pdf_tz_tests.rs"]
mod tz_tests;
