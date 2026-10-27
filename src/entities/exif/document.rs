//! 办公文档时间适配：调 `entities::office::populate_office_dates` 拿 `(created, modified)`
//! 二元组后填 `Exif.doc_created` / `Exif.doc_modified`。
//!

use chrono::FixedOffset;

use super::super::backend::MediaReader;
use super::super::office;
use super::types::Exif;

pub(super) fn populate_document_dates(
    reader: &mut dyn MediaReader,
    mime: &str,
    exif: &mut Exif,
    default_offset: FixedOffset,
) {
    let (created, modified) = office::populate_office_dates(reader, mime, default_offset);
    exif.set_doc_created(created);
    exif.set_doc_modified(modified);
}
