use chrono::FixedOffset;
use nom_exif::TrackInfoTag;

use super::super::backend::MediaReader;
use super::exif_io;
use super::types::Exif;
use super::types::entry_value_to_epoch;

pub(super) fn populate_avi_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Some(avi) = crate::entities::riff::parse_avi_exif(reader.as_mut()) else {
        return;
    };
    exif.date_time_original = avi
        .date_time_original
        .as_deref()
        .map_or(0, |s| ascii_datetime_to_epoch(s, local_offset));
    exif.create_date = avi
        .create_date
        .as_deref()
        .map_or(0, |s| ascii_datetime_to_epoch(s, local_offset));
    exif.modify_date = avi
        .modify_date
        .as_deref()
        .map_or(0, |s| ascii_datetime_to_epoch(s, local_offset));
    exif.make = avi.make;
    exif.model = avi.model;
}

pub(super) fn populate_m2ts_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Some(dt) = crate::entities::m2ts::parse_m2ts_datetime(reader.as_mut()) else {
        return;
    };
    exif.date_time_original = ascii_datetime_to_epoch(&dt, local_offset);
}

pub(super) fn ascii_datetime_to_epoch(s: &str, local_offset: FixedOffset) -> u64 {
    chrono::NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .ok()
        .and_then(|nd| nd.and_local_timezone(local_offset).single())
        .map_or(0, |dt| {
            let secs = dt.timestamp();
            if secs <= 0 { 0 } else { secs.cast_unsigned() }
        })
}

pub(super) fn populate_video_dates(
    reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Some(track) = exif_io::parse_video_track(reader) else {
        return;
    };
    if let Some(v) = track.get(TrackInfoTag::CreateDate) {
        exif.qt_create_date = entry_value_to_epoch(v, local_offset);
    }
}
