//! EPUB `META-INF/container.xml` → OPF `dc:date` / `dcterms:modified` 解析。
//!
//! EPUB 是 zip 容器：
//! 1. `META-INF/container.xml` 含 `<rootfile full-path="OEBPS/content.opf" .../>` 指向 OPF 文件
//! 2. OPF 含 `<dc:date>2017-02-14T10:30:00Z</dc:date>`（创建时间）+
//!    `<meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>`（最后修改时间）

use std::io::Read;

use chrono::DateTime;

use crate::entities::backend::MediaReader;

const CONTAINER_XML: &str = "META-INF/container.xml";
const ENTRY_MAX_BYTES: usize = 64 * 1024;
const ATTR_FULL_PATH: &[u8] = b"full-path=\"";
const TAG_DC_DATE_OPEN: &[u8] = b"<dc:date";
const TAG_DC_DATE_CLOSE: &[u8] = b"</dc:date>";
const TAG_META_CLOSE: &[u8] = b"</meta>";
const PROPERTY_DCTERMS_MODIFIED: &[u8] = b"dcterms:modified";

/// 入口：把 reader 当 zip 容器打开，双跳读 container.xml → OPF。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return (0, 0);
    };
    let Some(container) = read_entry(&mut archive, CONTAINER_XML) else {
        return (0, 0);
    };
    let Some(opf_path) = find_opf_path(&container) else {
        return (0, 0);
    };
    let Some(opf) = read_entry(&mut archive, &opf_path) else {
        return (0, 0);
    };
    extract_dates(&opf)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    let Ok(entry) = archive.by_name(name) else {
        return None;
    };
    let mut content = Vec::with_capacity(ENTRY_MAX_BYTES);
    if entry
        .take(ENTRY_MAX_BYTES as u64)
        .read_to_end(&mut content)
        .is_err()
    {
        return None;
    }
    Some(content)
}

/// 在 container.xml 字节内找 `<rootfile full-path="...">` 的 OPF 路径。
pub(super) fn find_opf_path(buf: &[u8]) -> Option<String> {
    let start = find_subslice(buf, ATTR_FULL_PATH)?;
    let after = &buf[start + ATTR_FULL_PATH.len()..];
    let end = find_byte(after, b'"')?;
    std::str::from_utf8(&after[..end]).ok().map(str::to_owned)
}

/// 纯字节扫描业务：从 OPF 内容查 `dc:date` element + `meta property="dcterms:modified"`。
pub(super) fn extract_dates(buf: &[u8]) -> (u64, u64) {
    let created = scan_element_text(buf, TAG_DC_DATE_OPEN, TAG_DC_DATE_CLOSE)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    let modified = scan_meta_property(buf, PROPERTY_DCTERMS_MODIFIED)
        .and_then(parse_iso8601_to_epoch)
        .unwrap_or(0);
    (created, modified)
}

/// 找 `<meta property="<key>">value</meta>`。属性顺序可能 `id` 在前，但 `property=`
/// 必出现；这里直接搜 `property="<key>"` 然后取 `>` 后内容至 `</meta>`。
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
#[path = "epub_tests.rs"]
mod tests;
