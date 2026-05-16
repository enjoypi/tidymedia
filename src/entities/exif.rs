use std::fs;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use camino::Utf8Path;
use nom_exif::EntryValue;
use nom_exif::ExifTag;
use nom_exif::TrackInfoTag;

use super::common;

const META_TYPE_IMAGE: &str = "image/";
const META_TYPE_VIDEO: &str = "video/";

#[derive(Clone, Debug, Default)]
pub struct Exif {
    mime_type: String,

    file_modify_date: u64,
    file_create_date: u64,

    exif_create_date: u64,
    exif_modify_date: u64,
    date_time_original: u64,

    // exiftool 历史把 MP4 box 命名为 H264:*；nom-exif 不区分来源、统一走 qt_* 字段。
    // 字段保留为 0，以保持对外 API 兼容。
    h264_date_time_original: u64,
    qt_media_create_date: u64,
    qt_create_date: u64,
}

impl Exif {
    pub fn from_path(path: &Utf8Path) -> common::Result<Self> {
        let meta = fs::metadata(path)?;

        let mime_type = infer::get_from_path(path)?
            .map(|t| t.mime_type().to_string())
            .unwrap_or_default();

        let mut exif = Exif {
            mime_type,
            file_modify_date: system_time_to_epoch(meta.modified().ok()),
            file_create_date: system_time_to_epoch(meta.created().ok()),
            ..Default::default()
        };

        if exif.mime_type.starts_with(META_TYPE_IMAGE) {
            populate_image_dates(path, &mut exif);
        } else if exif.mime_type.starts_with(META_TYPE_VIDEO) {
            populate_video_dates(path, &mut exif);
        }

        Ok(exif)
    }

    pub fn mime_type(&self) -> &str {
        self.mime_type.as_str()
    }

    pub fn file_modify_date(&self) -> u64 {
        self.file_modify_date
    }

    pub fn file_create_date(&self) -> u64 {
        self.file_create_date
    }

    pub fn exif_create_date(&self) -> u64 {
        self.exif_create_date
    }

    pub fn exif_modify_date(&self) -> u64 {
        self.exif_modify_date
    }

    pub fn date_time_original(&self) -> u64 {
        self.date_time_original
    }

    pub fn h264_date_time_original(&self) -> u64 {
        self.h264_date_time_original
    }

    pub fn qt_media_create_date(&self) -> u64 {
        self.qt_media_create_date
    }

    pub fn qt_create_date(&self) -> u64 {
        self.qt_create_date
    }

    pub fn media_create_date(&self) -> u64 {
        if !self.is_media() {
            return 0;
        }

        if self.date_time_original() > 0 {
            return self.date_time_original();
        }

        if self.h264_date_time_original() > 0 {
            return self.h264_date_time_original();
        }

        if self.qt_media_create_date() > 0 {
            return self.qt_media_create_date();
        }

        if self.qt_create_date() > 0 {
            return self.qt_create_date();
        }

        if self.exif_create_date() > 0 {
            return self.exif_create_date();
        }

        if self.exif_modify_date() > 0 {
            return self.exif_modify_date();
        }

        if self.file_modify_date() > self.file_create_date() && self.file_create_date() > 0 {
            return self.file_create_date();
        }

        if self.file_modify_date() > 0 {
            return self.file_modify_date();
        }

        if self.file_create_date() > 0 {
            return self.file_create_date();
        }

        0
    }

    pub fn is_media(&self) -> bool {
        let mime_type = self.mime_type();
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !mime_type.ends_with(".fpx")
    }
}

fn system_time_to_epoch(t: Option<SystemTime>) -> u64 {
    t.and_then(|s| s.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn entry_value_to_epoch(v: &EntryValue) -> u64 {
    let secs = match v {
        EntryValue::DateTime(dt) => dt.timestamp(),
        EntryValue::NaiveDateTime(nd) => nd.and_utc().timestamp(),
        _ => return 0,
    };
    if secs <= 0 {
        0
    } else {
        secs as u64
    }
}

fn populate_image_dates(path: &Utf8Path, exif: &mut Exif) {
    let Ok(parsed) = nom_exif::read_exif(path.as_str()) else {
        return;
    };
    if let Some(v) = parsed.get(ExifTag::DateTimeOriginal) {
        exif.date_time_original = entry_value_to_epoch(v);
    }
    if let Some(v) = parsed.get(ExifTag::CreateDate) {
        exif.exif_create_date = entry_value_to_epoch(v);
    }
    if let Some(v) = parsed.get(ExifTag::ModifyDate) {
        exif.exif_modify_date = entry_value_to_epoch(v);
    }
}

fn populate_video_dates(path: &Utf8Path, exif: &mut Exif) {
    let Ok(track) = nom_exif::read_track(path.as_str()) else {
        return;
    };
    if let Some(v) = track.get(TrackInfoTag::CreateDate) {
        exif.qt_create_date = entry_value_to_epoch(v);
    }
}

#[cfg(test)]
impl Exif {
    /// 跨模块测试用：根据 MIME 构造一个除 mime_type 外全部为 0 的 Exif。
    pub(crate) fn with_mime(mime_type: &str) -> Self {
        Self {
            mime_type: mime_type.to_string(),
            ..Default::default()
        }
    }

    /// 跨模块测试用：链式设置 EXIF 拍摄时间。
    pub(crate) fn with_date_time_original(mut self, secs: u64) -> Self {
        self.date_time_original = secs;
        self
    }
}

#[cfg(test)]
#[path = "exif_tests.rs"]
mod tests;
