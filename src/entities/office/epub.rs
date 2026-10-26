use chrono::DateTime;

use crate::entities::backend::MediaReader;

use super::zip_io;

const CONTAINER_XML: &str = "META-INF/container.xml";
const ENTRY_MAX_BYTES: usize = 64 * 1024;
const ATTR_FULL_PATH: &[u8] = b"full-path=\"";
const TAG_DC_DATE_OPEN: &[u8] = b"<dc:date";
const TAG_DC_DATE_CLOSE: &[u8] = b"</dc:date>";
const TAG_META_CLOSE: &[u8] = b"</meta>";
const PROPERTY_DCTERMS_MODIFIED: &[u8] = b"dcterms:modified";

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return (0, 0);
    };
    let Some(container) = zip_io::read_entry(&mut archive, CONTAINER_XML, ENTRY_MAX_BYTES as u64)
    else {
        return (0, 0);
    };
    let Some(opf_path) = find_opf_path(&container) else {
        return (0, 0);
    };
    let Some(opf) = zip_io::read_entry(&mut archive, &opf_path, ENTRY_MAX_BYTES as u64) else {
        return (0, 0);
    };
    extract_dates(&opf)
}

const CHAPTER_MAX_BYTES: u64 = 128 * 1024;

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return String::new();
    };
    let mut names: Vec<String> = zip_io::entry_names(&archive)
        .into_iter()
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    e.eq_ignore_ascii_case("xhtml")
                        || e.eq_ignore_ascii_case("html")
                        || e.eq_ignore_ascii_case("htm")
                })
        })
        .collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        if out.len() >= max_bytes {
            break;
        }
        let Some(content) = zip_io::read_entry(&mut archive, &name, CHAPTER_MAX_BYTES) else {
            continue;
        };
        super::scan::strip_markup_into(&content, &mut out, max_bytes);
    }
    out
}

pub(super) fn find_opf_path(buf: &[u8]) -> Option<String> {
    let start = find_subslice(buf, ATTR_FULL_PATH)?;
    let after = &buf[start + ATTR_FULL_PATH.len()..];
    let end = find_byte(after, b'"')?;
    std::str::from_utf8(&after[..end]).ok().map(str::to_owned)
}

pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_DC_DATE_OPEN, TAG_DC_DATE_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    let modified = scan_meta_property(buf, PROPERTY_DCTERMS_MODIFIED)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    (created, modified)
}

fn scan_meta_property<'a>(buf: &'a [u8], property_key: &[u8]) -> Option<&'a str> {
    let mut needle: Vec<u8> = b"property=\"".to_vec();
    needle.extend_from_slice(property_key);
    needle.push(b'"');
    let start = find_subslice(buf, &needle)?;
    let after = &buf[start + needle.len()..];
    let gt = find_byte(after, b'>')?;
    let text_start_in_after = gt + 1;
    let text = &after[text_start_in_after..];
    let end = find_subslice(text, TAG_META_CLOSE)?;
    std::str::from_utf8(&text[..end]).ok()
}

fn scan_element_text<'a>(buf: &'a [u8], open_tag: &[u8], close_tag: &[u8]) -> Option<&'a str> {
    let start = find_subslice(buf, open_tag)?;
    let after_open = start + open_tag.len();
    let rest = &buf[after_open..];
    let gt = find_byte(rest, b'>')?;
    let text_start = after_open + gt + 1;
    let text = &buf[text_start..];
    let end = find_subslice(text, close_tag)?;
    std::str::from_utf8(&text[..end]).ok()
}

fn parse_iso8601_to_epoch(s: &str) -> Option<u64> {
    let dt = DateTime::parse_from_rfc3339(s.trim()).ok()?;
    let secs = dt.timestamp();
    if secs > 0 {
        Some(secs.cast_unsigned())
    } else {
        None
    }
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
#[path = "epub_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "epub_tests.rs"]
mod tests;
