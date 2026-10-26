use std::io::{Read, Seek};

use crate::entities::backend::MediaReader;
use crate::entities::office::scan::{strip_markup_into, truncate_at_boundary};

use super::super::zip_io;
use super::{
    CONTENT_JSON, CONTENT_MAX_BYTES, CONTENT_XML, JSON_MAX_BYTES, METADATA_JSON, MIME_XMIND,
    MIME_XMIND_ALT, XmindMetadata,
};

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return (0, 0);
    };
    if mime == MIME_XMIND || mime == MIME_XMIND_ALT {
        parse_xmind(&mut archive)
    } else {
        (0, 0)
    }
}

#[doc(hidden)]
pub fn parse_xmind<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> (u64, u64) {
    let Some(content) = zip_io::read_entry(archive, METADATA_JSON, JSON_MAX_BYTES as u64) else {
        return (0, 0);
    };
    extract_dates_from_json(&content)
}

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(xml) = read_entry_capped(&mut archive, CONTENT_XML) {
        strip_markup_into(&xml, &mut out, max_bytes);
    } else if let Some(json) = read_entry_capped(&mut archive, CONTENT_JSON) {
        collect_json_titles(&json, &mut out, max_bytes);
    }
    out
}

#[doc(hidden)]
pub fn read_entry_capped<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    zip_io::read_entry(archive, name, CONTENT_MAX_BYTES)
}

#[doc(hidden)]
pub fn collect_json_titles(buf: &[u8], out: &mut String, max_bytes: usize) {
    const KEY: &[u8] = b"\"title\":";
    let mut pos = 0;
    while out.len() < max_bytes {
        let Some(rel) = find_subslice(&buf[pos..], KEY) else {
            break;
        };
        let mut i = pos + rel + KEY.len();
        while i < buf.len() && buf[i].is_ascii_whitespace() {
            i += 1;
        }
        if buf.get(i) != Some(&b'"') {
            pos = i;
            continue;
        }
        i += 1;
        let start = i;
        while i < buf.len() && buf[i] != b'"' {
            if buf[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        let val = &buf[start..i.min(buf.len())];
        if !val.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&String::from_utf8_lossy(val));
            truncate_at_boundary(out, max_bytes);
        }
        pos = i;
    }
}

#[doc(hidden)]
#[must_use]
pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[doc(hidden)]
pub fn extract_dates_from_json(buf: &[u8]) -> (u64, u64) {
    let Ok(meta) = serde_json::from_slice::<XmindMetadata>(buf) else {
        return (0, 0);
    };
    let created = meta.created.and_then(millis_to_secs).unwrap_or(0);
    let modified = meta.modified.and_then(millis_to_secs).unwrap_or(0);
    (created, modified)
}

#[doc(hidden)]
#[must_use]
pub fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1000;
    if secs < 86_400 { None } else { Some(secs) }
}
