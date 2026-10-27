use chrono::FixedOffset;

use crate::entities::backend::MediaReader;

pub mod cfb;
mod cfb_io;
pub(crate) mod epub;
mod flate_io;
pub(crate) mod iwork;
pub(crate) mod mindmap_mm;
pub mod mindmap_zip;
pub(crate) mod odf;
pub(crate) mod ooxml;
pub(crate) mod pdf;
mod plist_io;
mod read_io;
pub(crate) mod rtf;
pub(crate) mod scan;
pub(crate) mod text;
mod zip_io;

pub(crate) const MIME_PDF: &str = "application/pdf";
pub(crate) const MIME_DOCX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub(crate) const MIME_PPTX: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";
pub(crate) const MIME_XLSX: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
pub(crate) const MIME_OOXML_PREFIX: &str = "application/vnd.openxmlformats-officedocument.";
pub(crate) const MIME_DOC: &str = "application/msword";
pub(crate) const MIME_PPT: &str = "application/vnd.ms-powerpoint";
pub(crate) const MIME_XLS: &str = "application/vnd.ms-excel";
pub(crate) const MIME_PAGES: &str = "application/vnd.apple.pages";
pub(crate) const MIME_NUMBERS: &str = "application/vnd.apple.numbers";
pub(crate) const MIME_KEYNOTE: &str = "application/vnd.apple.keynote";
pub(crate) const MIME_IWORK_PREFIX: &str = "application/x-iwork-";
pub(crate) const MIME_ODT: &str = "application/vnd.oasis.opendocument.text";
pub(crate) const MIME_ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";
pub(crate) const MIME_ODP: &str = "application/vnd.oasis.opendocument.presentation";
pub(crate) const MIME_ODG: &str = "application/vnd.oasis.opendocument.graphics";
pub(crate) const MIME_ODF_PREFIX: &str = "application/vnd.oasis.opendocument.";
pub(crate) const MIME_RTF_APP: &str = "application/rtf";
pub(crate) const MIME_RTF_TEXT: &str = "text/rtf";
pub(crate) const MIME_EPUB: &str = "application/epub+zip";
pub(crate) const MIME_XMIND: &str = "application/vnd.xmind.workbook";
pub(crate) const MIME_XMIND_ALT: &str = "application/x-xmind";
pub(crate) const MIME_FREEMIND: &str = "application/x-freemind";
pub(crate) const MIME_MINDNODE: &str = "application/x-mindnode";
pub(crate) const MIME_ITMZ: &str = "application/x-itmz";
pub(crate) const MIME_MINDMANAGER: &str = "application/x-mindmanager";
pub(crate) const MIME_OLE_STORAGE: &str = "application/x-ole-storage";
pub(crate) const MIME_TEXT_PLAIN: &str = "text/plain";
pub(crate) const MIME_TEXT_MARKDOWN: &str = "text/markdown";
pub(crate) const MIME_TEXT_RST: &str = "text/x-rst";
pub(crate) const MIME_TEXT_CSV: &str = "text/csv";
pub(crate) const MIME_TEXT_TSV: &str = "text/tab-separated-values";

#[doc(hidden)]
pub fn populate_office_dates(
    reader: &mut dyn MediaReader,
    mime: &str,
    default_offset: FixedOffset,
) -> (u64, u64) {
    if mime.starts_with(MIME_PDF) {
        pdf::parse(reader, mime, default_offset)
    } else if is_ooxml_mime(mime) {
        ooxml::parse(reader, mime)
    } else if is_cfb_mime(mime) {
        cfb::parse(reader, mime)
    } else if is_iwork_mime(mime) {
        iwork::parse(reader, mime)
    } else if mime.starts_with(MIME_ODF_PREFIX) {
        odf::parse(reader, mime)
    } else if mime == MIME_RTF_APP || mime == MIME_RTF_TEXT {
        rtf::parse(reader, mime)
    } else if mime == MIME_EPUB {
        epub::parse(reader, mime)
    } else if is_mindmap_zip_mime(mime) {
        mindmap_zip::parse(reader, mime)
    } else if mime == MIME_FREEMIND {
        mindmap_mm::parse(reader, mime)
    } else {
        text::parse(reader, mime)
    }
}

#[doc(hidden)]
pub fn extract_office_text(reader: &mut dyn MediaReader, mime: &str, max_bytes: usize) -> String {
    if mime.starts_with(MIME_PDF) {
        pdf::extract_text(reader, mime, max_bytes)
    } else if is_ooxml_mime(mime) {
        ooxml::extract_text(reader, mime, max_bytes)
    } else if is_cfb_mime(mime) {
        cfb::extract_text(reader, mime, max_bytes)
    } else if is_iwork_mime(mime) {
        String::new()
    } else if mime.starts_with(MIME_ODF_PREFIX) {
        odf::extract_text(reader, mime, max_bytes)
    } else if mime == MIME_RTF_APP || mime == MIME_RTF_TEXT {
        rtf::extract_text(reader, mime, max_bytes)
    } else if mime == MIME_EPUB {
        epub::extract_text(reader, mime, max_bytes)
    } else if is_mindmap_zip_mime(mime) {
        mindmap_zip::extract_text(reader, mime, max_bytes)
    } else if mime == MIME_FREEMIND {
        mindmap_mm::extract_text(reader, mime, max_bytes)
    } else {
        text::extract_text(reader, mime, max_bytes)
    }
}

fn is_ooxml_mime(mime: &str) -> bool {
    mime.starts_with(MIME_OOXML_PREFIX)
}

fn is_cfb_mime(mime: &str) -> bool {
    if mime == MIME_DOC {
        return true;
    }
    if mime == MIME_PPT {
        return true;
    }
    mime == MIME_XLS
}

fn is_iwork_mime(mime: &str) -> bool {
    mime == MIME_PAGES
        || mime == MIME_NUMBERS
        || mime == MIME_KEYNOTE
        || mime.starts_with(MIME_IWORK_PREFIX)
}

fn is_mindmap_zip_mime(mime: &str) -> bool {
    mime == MIME_XMIND
        || mime == MIME_XMIND_ALT
        || mime == MIME_MINDNODE
        || mime == MIME_ITMZ
        || mime == MIME_MINDMANAGER
}

#[cfg(test)]
#[path = "mod_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "office_tests_common.rs"]
pub(crate) mod tests_common;
