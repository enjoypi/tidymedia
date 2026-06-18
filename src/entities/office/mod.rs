//! 办公文档时间解析路由。
//!
//! 按 MIME 分流到子模块（PDF / OOXML / CFB / iWork / ODF / RTF / EPUB / 思维导图 / 纯文本）。
//! 各子模块返回 `(created_epoch, modified_epoch)` 二元组，0 表字段缺失。
//! 上层 `entities::exif::document::populate_document_dates` 调本 fn 后填 `Exif.doc_created`
//! / `Exif.doc_modified`，下游 `Info::create_time` 把非零 `doc_created` 注入 P0
//! [`crate::entities::media_time::Source::DocumentCreated`] 候选。

// 占位实现：commit 2-10 接入各子模块解析主体后逐个移除本节 allow；首版 stub
// 让装配链路（types.rs 分流 + Source 注入 + e2e fixture）先编译通过。
#![allow(dead_code, reason = "占位实现：commit 2-10 接入各子模块解析主体后移除")]

use crate::entities::backend::MediaReader;

pub(crate) mod cfb;
pub(crate) mod epub;
pub(crate) mod iwork;
pub(crate) mod mindmap_mm;
pub(crate) mod mindmap_zip;
pub(crate) mod odf;
pub(crate) mod ooxml;
pub(crate) mod pdf;
pub(crate) mod rtf;
pub(crate) mod text;

// MIME 常量集中此处避免与 exif/mime.rs 双向引用循环。`mime_from_ext` 会用同套
// 字面量做扩展名→MIME 反查；新增容器 MUST 在两处同步（CLAUDE.md「同步检查点」）。
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
pub(crate) const MIME_TEXT_PLAIN: &str = "text/plain";
pub(crate) const MIME_TEXT_MARKDOWN: &str = "text/markdown";
pub(crate) const MIME_TEXT_RST: &str = "text/x-rst";
pub(crate) const MIME_TEXT_CSV: &str = "text/csv";
pub(crate) const MIME_TEXT_TSV: &str = "text/tab-separated-values";

/// 路由入口：按 mime 分流到子解析器，返 `(created_epoch, modified_epoch)`；0 表缺失。
///
/// 接 `&mut dyn MediaReader` 而非 `Box<dyn MediaReader>`：让 stub 阶段子模块不消费
/// reader 不触发 `clippy::needless_pass_by_value`；commit 2-9 接入主体时（zip /
/// pdf 字节读 / cfb 容器读取）`&mut Read + Seek` 仍是合法 trait bound。
pub(crate) fn populate_office_dates(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    if mime.starts_with(MIME_PDF) {
        pdf::parse(reader, mime)
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
        // 纯文本族 text/plain text/markdown text/csv 等：无 metadata，由调用方退到 P2/P4。
        text::parse(reader, mime)
    }
}

fn is_ooxml_mime(mime: &str) -> bool {
    mime.starts_with(MIME_OOXML_PREFIX)
}

fn is_cfb_mime(mime: &str) -> bool {
    mime == MIME_DOC || mime == MIME_PPT || mime == MIME_XLS
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
#[path = "mod_tests.rs"]
mod tests;
