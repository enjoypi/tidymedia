use std::sync::Arc;

use camino::Utf8Path;
use chrono::FixedOffset;

use super::Exif;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::uri::Location;

pub(super) fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

pub(super) fn mk_exif(mime: &str, init: impl FnOnce(&mut Exif)) -> Exif {
    let mut exif = Exif {
        mime_type: mime.to_string(),
        ..Default::default()
    };
    init(&mut exif);
    exif
}

impl Exif {
    pub fn from_path_with_offset(
        path: &Utf8Path,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let backend = LocalBackend::arc();
        Self::open(&Location::Local(path.to_path_buf()), &backend, local_offset)
    }

    pub fn open(
        loc: &Location,
        backend: &Arc<dyn Backend>,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        Self::open_filtered(loc, backend, local_offset, true, false)
    }

    pub(crate) fn from_path(path: &Utf8Path) -> common::Result<Self> {
        Self::from_path_with_offset(path, FixedOffset::east_opt(0).expect("UTC offset is valid"))
    }

    pub(crate) fn with_mime(mime_type: &str) -> Self {
        Self {
            mime_type: mime_type.to_string(),
            ..Default::default()
        }
    }

    pub(crate) fn with_date_time_original(mut self, secs: u64) -> Self {
        self.date_time_original = secs;
        self
    }

    pub(crate) fn with_qt_create_date(mut self, secs: u64) -> Self {
        self.qt_create_date = secs;
        self
    }

    pub(crate) fn with_doc_created(mut self, secs: u64) -> Self {
        self.doc_created = secs;
        self
    }

    pub(crate) fn with_make_model(mut self, make: &str, model: &str) -> Self {
        self.make = Some(make.to_string());
        self.model = Some(model.to_string());
        self
    }
}
