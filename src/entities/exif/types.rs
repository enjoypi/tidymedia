use std::sync::Arc;

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Utc;
use nom_exif::EntryValue;

#[path = "route.rs"]
mod route;

use super::super::backend::Backend;
use super::super::common;
use super::super::uri::Location;
use super::mime::META_TYPE_IMAGE;
use super::mime::META_TYPE_VIDEO;
use super::mime::is_office_mime;
use super::mime::sniff_mime;

#[derive(Clone, Debug, Default)]
pub struct Exif {
    pub(super) mime_type: String,

    pub(super) create_date: u64,
    pub(super) date_time_original: u64,
    pub(super) modify_date: u64,

    pub(super) qt_create_date: u64,

    pub(super) gps_utc: Option<DateTime<Utc>>,

    pub(super) make: Option<String>,
    pub(super) model: Option<String>,

    pub(super) doc_created: u64,
    pub(super) doc_modified: u64,
}

impl Exif {
    pub fn open_filtered(
        loc: &Location,
        backend: &Arc<dyn Backend>,
        local_offset: FixedOffset,
        parse_non_media: bool,
        doc_only: bool,
    ) -> common::Result<Self> {
        let mut reader = backend.open_read(loc)?;
        let sniffed = sniff_mime(reader.as_mut())?;
        let needs_ext_remap = sniffed.is_empty()
            || sniffed == "application/zip"
            || sniffed == crate::entities::office::MIME_OLE_STORAGE;
        let mime_type = if needs_ext_remap {
            super::mime::mime_from_ext(loc.path().extension())
                .map_or_else(|| sniffed.clone(), str::to_string)
        } else {
            sniffed
        };
        let parse_full = if doc_only {
            is_office_mime(&mime_type)
        } else if parse_non_media {
            true
        } else {
            Self::is_media_mime(&mime_type)
        };
        if !parse_full {
            return Ok(Self {
                mime_type,
                ..Self::default()
            });
        }
        Ok(Self::from_reader(reader, &mime_type, local_offset))
    }

    pub fn from_reader(
        reader: Box<dyn crate::entities::backend::MediaReader>,
        mime_type: &str,
        local_offset: FixedOffset,
    ) -> Self {
        route::route_reader(reader, mime_type, local_offset)
    }

    pub fn mime_type(&self) -> &str {
        self.mime_type.as_str()
    }

    pub fn exif_create_date(&self) -> u64 {
        self.create_date
    }

    pub fn exif_modify_date(&self) -> u64 {
        self.modify_date
    }

    pub fn date_time_original(&self) -> u64 {
        self.date_time_original
    }

    pub fn qt_create_date(&self) -> u64 {
        self.qt_create_date
    }

    pub fn doc_created(&self) -> u64 {
        self.doc_created
    }

    pub(super) fn set_doc_created(&mut self, secs: u64) {
        self.doc_created = secs;
    }

    pub(super) fn set_doc_modified(&mut self, secs: u64) {
        self.doc_modified = secs;
    }

    pub fn gps_utc(&self) -> Option<DateTime<Utc>> {
        self.gps_utc
    }

    pub fn is_mkv_container(&self) -> bool {
        if self.mime_type.starts_with("video/x-matroska") {
            return true;
        }
        self.mime_type.starts_with("video/webm")
    }

    pub fn make(&self) -> Option<&str> {
        self.make.as_deref()
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn is_media(&self) -> bool {
        Self::is_media_mime(self.mime_type())
    }

    pub fn is_office(&self) -> bool {
        is_office_mime(self.mime_type())
    }

    #[must_use]
    pub fn is_media_mime(mime_type: &str) -> bool {
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !camino::Utf8Path::new(mime_type)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("fpx"))
    }
}

pub(super) fn entry_value_to_epoch(v: &EntryValue, local_offset: FixedOffset) -> u64 {
    let secs = match v {
        EntryValue::DateTime(dt) => dt.timestamp(),
        EntryValue::NaiveDateTime(nd) => nd
            .and_local_timezone(local_offset)
            .single()
            .map_or(0, |x| x.timestamp()),
        _ => return 0,
    };
    if secs <= 0 { 0 } else { secs.cast_unsigned() }
}
