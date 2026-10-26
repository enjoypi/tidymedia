use chrono::{DateTime, NaiveDateTime};

use crate::entities::backend::MediaReader;

use super::zip_io;

const META_XML: &str = "meta.xml";
const TAG_CREATED_OPEN: &[u8] = b"<meta:creation-date";
const TAG_CREATED_CLOSE: &[u8] = b"</meta:creation-date>";
const TAG_DC_DATE_OPEN: &[u8] = b"<dc:date";
const TAG_DC_DATE_CLOSE: &[u8] = b"</dc:date>";

const META_XML_MAX_BYTES: usize = 64 * 1024;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return (0, 0);
    };
    let Some(content) = zip_io::read_entry(&mut archive, META_XML, META_XML_MAX_BYTES as u64)
    else {
        return (0, 0);
    };
    extract_dates(&content)
}

const CONTENT_XML: &str = "content.xml";
const CONTENT_XML_MAX_BYTES: u64 = 256 * 1024;

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return String::new();
    };
    let Some(content) = zip_io::read_entry(&mut archive, CONTENT_XML, CONTENT_XML_MAX_BYTES) else {
        return String::new();
    };
    let mut out = String::new();
    super::scan::strip_markup_into(&content, &mut out, max_bytes);
    out
}

#[doc(hidden)]
pub fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_CREATED_OPEN, TAG_CREATED_CLOSE)
        .and_then(parse_odf_datetime)
        .unwrap_or(0);
    let modified = scan_element_text(buf, TAG_DC_DATE_OPEN, TAG_DC_DATE_CLOSE)
        .and_then(parse_odf_datetime)
        .unwrap_or(0);
    (created, modified)
}

#[doc(hidden)]
#[must_use]
pub fn parse_odf_datetime(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        let secs = dt.timestamp();
        if secs > 0 {
            return Some(secs.cast_unsigned());
        }
    }
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S").ok()?;
    let dt = naive.and_utc();
    let secs = dt.timestamp();
    if secs > 0 {
        Some(secs.cast_unsigned())
    } else {
        None
    }
}

#[doc(hidden)]
#[must_use]
pub fn scan_element_text<'a>(buf: &'a [u8], open_tag: &[u8], close_tag: &[u8]) -> Option<&'a str> {
    let start = find_subslice(buf, open_tag)?;
    let after_open = start + open_tag.len();
    let rest = &buf[after_open..];
    let gt = find_byte(rest, b'>')?;
    let text_start = after_open + gt + 1;
    let text = &buf[text_start..];
    let end = find_subslice(text, close_tag)?;
    std::str::from_utf8(&text[..end]).ok()
}

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

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "odf_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "odf_tests.rs"]
mod tests;
