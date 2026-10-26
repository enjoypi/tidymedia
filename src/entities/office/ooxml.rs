use chrono::DateTime;

use crate::entities::backend::MediaReader;

use super::zip_io;

const CORE_XML: &str = "docProps/core.xml";
const TAG_CREATED_OPEN: &[u8] = b"<dcterms:created";
const TAG_CREATED_CLOSE: &[u8] = b"</dcterms:created>";
const TAG_MODIFIED_OPEN: &[u8] = b"<dcterms:modified";
const TAG_MODIFIED_CLOSE: &[u8] = b"</dcterms:modified>";
const CORE_XML_MAX_BYTES: usize = 64 * 1024;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return (0, 0);
    };
    let Some(content) = zip_io::read_entry(&mut archive, CORE_XML, CORE_XML_MAX_BYTES as u64)
    else {
        return (0, 0);
    };
    extract_dates(&content)
}

const DOCX_DOCUMENT_XML: &str = "word/document.xml";
const XLSX_SHARED_STRINGS: &str = "xl/sharedStrings.xml";
const PPTX_SLIDES_PREFIX: &str = "ppt/slides/slide";
const BODY_XML_MAX_BYTES: u64 = 256 * 1024;

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, mime: &str, max_bytes: usize) -> String {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return String::new();
    };
    let mut out = String::new();
    if mime == super::MIME_PPTX {
        let mut names: Vec<String> = zip_io::entry_names(&archive)
            .into_iter()
            .filter(|n| {
                n.starts_with(PPTX_SLIDES_PREFIX)
                    && std::path::Path::new(n)
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
            })
            .collect();
        names.sort();
        for name in names {
            if out.len() >= max_bytes {
                break;
            }
            append_entry_text(&mut archive, &name, &mut out, max_bytes);
        }
    } else if mime == super::MIME_XLSX {
        append_entry_text(&mut archive, XLSX_SHARED_STRINGS, &mut out, max_bytes);
    } else {
        append_entry_text(&mut archive, DOCX_DOCUMENT_XML, &mut out, max_bytes);
    }
    out
}

fn append_entry_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    out: &mut String,
    max_bytes: usize,
) {
    let Some(content) = zip_io::read_entry(archive, name, BODY_XML_MAX_BYTES) else {
        return;
    };
    super::scan::strip_markup_into(&content, out, max_bytes);
}

#[doc(hidden)]
pub fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_CREATED_OPEN, TAG_CREATED_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    let modified = scan_element_text(buf, TAG_MODIFIED_OPEN, TAG_MODIFIED_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    (created, modified)
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

#[doc(hidden)]
#[must_use]
pub fn parse_iso8601_to_epoch(s: &str) -> Option<u64> {
    let dt = DateTime::parse_from_rfc3339(s.trim()).ok()?;
    let secs = dt.timestamp();
    if secs <= 0 {
        None
    } else {
        Some(secs.cast_unsigned())
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
#[path = "ooxml_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "ooxml_tests.rs"]
mod tests;
