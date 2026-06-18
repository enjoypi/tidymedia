//! 办公文档时间适配：调 `entities::office::populate_office_dates` 拿 `(created, modified)`
//! 二元组后填 `Exif.doc_created` / `Exif.doc_modified`。
//!
//! office 文档不读 EXIF/XMP，故不需 `local_offset` —— 各容器内时间字段（dcterms:created
//! / PDF `/CreationDate` / CFB FILETIME / iWork Cocoa epoch / `.mm` Unix millis）
//! 都是带时区或 UTC 表示，子模块内部已经把字段值归一为 Unix UTC epoch。

use super::super::backend::MediaReader;
use super::super::office;
use super::types::Exif;

pub(super) fn populate_document_dates(reader: &mut dyn MediaReader, mime: &str, exif: &mut Exif) {
    let (created, modified) = office::populate_office_dates(reader, mime);
    exif.set_doc_created(created);
    exif.set_doc_modified(modified);
}
