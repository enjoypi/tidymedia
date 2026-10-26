use chrono::DateTime;
use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::Utc;
use nom_exif::ExifTag;
use nom_exif::URational;
use tracing::debug;

use super::super::backend::MediaReader;
use super::super::tiff_ifd::TiffIfd;
use super::super::xmp;
use super::exif_io;
use super::image_jpeg::parse_jpeg_app1_exif;
use super::types::Exif;
use super::types::entry_value_to_epoch;
use super::video::ascii_datetime_to_epoch;

const XMP_SCAN_BYTES: usize = 64 * 1024;

pub(super) fn populate_image_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let head = exif_io::read_head(reader.as_mut(), XMP_SCAN_BYTES);
    if exif_io::rewind(reader.as_mut()).is_err() {
        populate_image_from_head_only(&head, exif, local_offset);
        return;
    }
    let Some(parsed) = exif_io::parse_image_exif(reader) else {
        populate_image_from_head_only(&head, exif, local_offset);
        return;
    };
    if let Some(v) = parsed.get(ExifTag::DateTimeOriginal) {
        exif.date_time_original = entry_value_to_epoch(v, local_offset);
    }
    if let Some(v) = parsed.get(ExifTag::CreateDate) {
        exif.create_date = entry_value_to_epoch(v, local_offset);
    }
    exif.gps_utc = parse_gps_utc(&parsed);
    if let Some(v) = parsed.get(ExifTag::ModifyDate) {
        exif.modify_date = entry_value_to_epoch(v, local_offset);
    }
    exif.make = parsed
        .get(ExifTag::Make)
        .and_then(|v| v.as_str().map(str::to_owned));
    exif.model = parsed
        .get(ExifTag::Model)
        .and_then(|v| v.as_str().map(str::to_owned));
    populate_image_xmp_fallback_if_empty(&head, exif);
}

fn populate_image_from_head_only(head: &[u8], exif: &mut Exif, local_offset: FixedOffset) {
    if let Some(tiff) = parse_jpeg_app1_exif(head) {
        apply_tiff_ifd(exif, tiff, local_offset);
    }
    populate_image_xmp_fallback_if_empty(head, exif);
}

pub(super) fn populate_image_xmp_fallback_if_empty(head: &[u8], exif: &mut Exif) {
    if exif.date_time_original == 0 && exif.create_date == 0 {
        populate_image_xmp_fallback(head, exif);
    }
}

pub(super) fn apply_tiff_ifd(exif: &mut Exif, tiff: TiffIfd, local_offset: FixedOffset) {
    if let Some(s) = tiff.date_time_original.as_deref() {
        exif.date_time_original = ascii_datetime_to_epoch(s, local_offset);
    }
    if let Some(s) = tiff.create_date.as_deref() {
        exif.create_date = ascii_datetime_to_epoch(s, local_offset);
    }
    if let Some(s) = tiff.modify_date.as_deref() {
        exif.modify_date = ascii_datetime_to_epoch(s, local_offset);
    }
    exif.make = tiff.make;
    exif.model = tiff.model;
}

pub(super) fn populate_image_xmp_fallback(head: &[u8], exif: &mut Exif) {
    let Some(packet) = xmp::find_xmp_packet(head) else {
        return;
    };
    let dates = xmp::parse_xmp_dates(packet);
    if let Some(dt) = dates.photoshop_date_created {
        let secs = dt.timestamp();
        if secs > 0 {
            exif.date_time_original = secs.cast_unsigned();
        } else {
            debug!(
                feature = "exif",
                operation = "xmp_fallback",
                field = "DateTimeOriginal",
                value = %dt,
                secs,
                "xmp date predates Unix epoch; cannot encode in u64 field"
            );
        }
    }
    if let Some(dt) = dates.xmp_create_date {
        let secs = dt.timestamp();
        if secs > 0 {
            exif.create_date = secs.cast_unsigned();
        } else {
            debug!(
                feature = "exif",
                operation = "xmp_fallback",
                field = "CreateDate",
                value = %dt,
                secs,
                "xmp date predates Unix epoch; cannot encode in u64 field"
            );
        }
    }
}

const GPS_DATE_STAMP: u16 = ExifTag::GPSDateStamp.code();
const GPS_TIME_STAMP: u16 = ExifTag::GPSTimeStamp.code();

fn parse_gps_utc(parsed: &nom_exif::Exif) -> Option<DateTime<Utc>> {
    let mut date_str: Option<String> = None;
    let mut time_rationals: Option<[URational; 3]> = None;

    for entry in parsed.iter() {
        match entry.tag.code() {
            GPS_DATE_STAMP => {
                date_str = entry.value.as_str().map(str::to_owned);
            }
            GPS_TIME_STAMP => {
                time_rationals = entry
                    .value
                    .as_urational_slice()
                    .and_then(|s| <[URational; 3]>::try_from(s).ok())
                    .or(time_rationals);
            }
            _ => {}
        }
    }

    build_gps_utc(date_str.as_deref(), time_rationals)
}

pub(super) fn build_gps_utc(
    date_str: Option<&str>,
    time: Option<[URational; 3]>,
) -> Option<DateTime<Utc>> {
    let date = parse_gps_date(date_str?)?;
    let [h, m, s] = time?;
    let hour = rational_to_u32(h)?;
    let min = rational_to_u32(m)?;
    let sec = rational_to_u32(s)?;
    date.and_hms_opt(hour, min, sec).map(|ndt| ndt.and_utc())
}

pub(super) fn parse_gps_date(s: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let y: i32 = parts[0].trim().parse().ok()?;
    let mo: u32 = parts[1].trim().parse().ok()?;
    let d: u32 = parts[2].trim().parse().ok()?;
    NaiveDate::from_ymd_opt(y, mo, d)
}

pub(super) fn rational_to_u32(r: URational) -> Option<u32> {
    let denom = r.denominator();
    if denom == 0 {
        return None;
    }
    Some(r.numerator() / denom)
}
