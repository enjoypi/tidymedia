use std::io;

use super::super::backend::MediaReader;
use super::super::office;
use super::exif_io;

pub(super) const META_TYPE_IMAGE: &str = "image/";
pub(super) const META_TYPE_VIDEO: &str = "video/";
pub(super) const MIME_PNG: &str = "image/png";
pub(super) const MIME_RW2: &str = "image/x-panasonic-rw2";
pub(super) const MIME_AVI: &str = "video/x-msvideo";
const MIME_QUICKTIME: &str = "video/quicktime";
pub(super) const MIME_M2TS: &str = "video/m2ts";
const MIME_3GPP: &str = "video/3gpp";
const MIME_XAVC: &str = "video/mp4";

pub(super) fn sniff_mime(reader: &mut dyn MediaReader) -> io::Result<String> {
    let head = exif_io::sniff_window(reader)?;
    Ok(classify_head(&head))
}

fn classify_head(head: &[u8]) -> String {
    exif_io::infer_mime(head)
        .or_else(|| quicktime_legacy_mime(head).map(str::to_string))
        .or_else(|| m2ts_legacy_mime(head).map(str::to_string))
        .or_else(|| bmff_3gpp_mime(head).map(str::to_string))
        .or_else(|| bmff_xavc_mime(head).map(str::to_string))
        .unwrap_or_default()
}

pub(super) fn quicktime_legacy_mime(buf: &[u8]) -> Option<&'static str> {
    let head = buf.get(4..8)?;
    (head == b"pnot" || head == b"mdat").then_some(MIME_QUICKTIME)
}

pub(super) fn m2ts_legacy_mime(buf: &[u8]) -> Option<&'static str> {
    if buf.get(4) == Some(&0x47) && buf.get(196) == Some(&0x47) {
        Some(MIME_M2TS)
    } else {
        None
    }
}

pub(super) fn bmff_3gpp_mime(buf: &[u8]) -> Option<&'static str> {
    if buf.get(4..8) == Some(b"ftyp") && buf.get(8..11) == Some(b"3gp") {
        Some(MIME_3GPP)
    } else {
        None
    }
}

pub(super) fn bmff_xavc_mime(buf: &[u8]) -> Option<&'static str> {
    if buf.get(4..8) == Some(b"ftyp") && buf.get(8..12) == Some(b"XAVC") {
        Some(MIME_XAVC)
    } else {
        None
    }
}

pub(super) fn is_office_mime(mime: &str) -> bool {
    mime.starts_with(office::MIME_PDF)
        || mime.starts_with(office::MIME_OOXML_PREFIX)
        || mime == office::MIME_DOC
        || mime == office::MIME_PPT
        || mime == office::MIME_XLS
        || mime == office::MIME_PAGES
        || mime == office::MIME_NUMBERS
        || mime == office::MIME_KEYNOTE
        || mime.starts_with(office::MIME_IWORK_PREFIX)
        || mime.starts_with(office::MIME_ODF_PREFIX)
        || mime == office::MIME_RTF_APP
        || mime == office::MIME_RTF_TEXT
        || mime == office::MIME_EPUB
        || mime == office::MIME_XMIND
        || mime == office::MIME_XMIND_ALT
        || mime == office::MIME_FREEMIND
        || mime == office::MIME_MINDNODE
        || mime == office::MIME_ITMZ
        || mime == office::MIME_MINDMANAGER
        || mime == office::MIME_TEXT_PLAIN
        || mime == office::MIME_TEXT_MARKDOWN
        || mime == office::MIME_TEXT_RST
        || mime == office::MIME_TEXT_CSV
        || mime == office::MIME_TEXT_TSV
}

#[must_use]
pub(crate) fn mime_from_ext(ext: Option<&str>) -> Option<&'static str> {
    let lower = ext?.to_ascii_lowercase();
    let mime = match lower.as_str() {
        "rw2" => MIME_RW2,
        "pdf" => office::MIME_PDF,
        "docx" => office::MIME_DOCX,
        "pptx" => office::MIME_PPTX,
        "xlsx" => office::MIME_XLSX,
        "doc" => office::MIME_DOC,
        "ppt" => office::MIME_PPT,
        "xls" => office::MIME_XLS,
        "pages" => office::MIME_PAGES,
        "numbers" => office::MIME_NUMBERS,
        "key" => office::MIME_KEYNOTE,
        "odt" => office::MIME_ODT,
        "ods" => office::MIME_ODS,
        "odp" => office::MIME_ODP,
        "odg" => office::MIME_ODG,
        "rtf" => office::MIME_RTF_APP,
        "epub" => office::MIME_EPUB,
        "xmind" => office::MIME_XMIND,
        "mm" => office::MIME_FREEMIND,
        "mindnode" => office::MIME_MINDNODE,
        "itmz" => office::MIME_ITMZ,
        "mmap" => office::MIME_MINDMANAGER,
        "txt" | "log" => office::MIME_TEXT_PLAIN,
        "md" | "markdown" => office::MIME_TEXT_MARKDOWN,
        "rst" => office::MIME_TEXT_RST,
        "csv" => office::MIME_TEXT_CSV,
        "tsv" => office::MIME_TEXT_TSV,
        _ => return None,
    };
    Some(mime)
}
