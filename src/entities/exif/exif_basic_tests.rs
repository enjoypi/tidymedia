use camino::Utf8Path;
use chrono::FixedOffset;

use super::super::test_common as common;
use super::Exif;
use super::tests_common::mk_exif;
use super::tests_common::utc;

#[test]
fn from_path_reads_dns_benchmark_png() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_DNS_BENCHMARK)).unwrap();
    assert_eq!(exif.mime_type(), "image/png");
    assert!(exif.is_media());
    // PNG fixture 无 EXIF chunk → 容器内时间字段保持 0
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.qt_create_date(), 0);
}

#[test]
fn from_path_non_media_file() {
    // data_small 是随机二进制（非图片/视频），infer 应识别为非媒体
    let exif = Exif::from_path(Utf8Path::new(common::DATA_SMALL)).unwrap();
    assert!(!exif.is_media());
}

#[test]
fn from_path_missing_returns_err() {
    let err = Exif::from_path(Utf8Path::new("/definitely/missing/xyz.png")).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("IO"), "got: {msg}");
}

#[test]
fn is_media_image_true() {
    assert!(mk_exif("image/jpeg", |_| {}).is_media());
}

#[test]
fn is_media_video_true() {
    assert!(mk_exif("video/mp4", |_| {}).is_media());
}

#[test]
fn is_media_fpx_excluded() {
    assert!(!mk_exif("image/vnd.fpx", |_| {}).is_media());
}

#[test]
fn is_media_empty_mime_false() {
    assert!(!mk_exif("", |_| {}).is_media());
}

#[test]
fn accessors_return_zero_for_missing_fields() {
    let exif = mk_exif("", |_| {});
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.qt_create_date(), 0);
    assert_eq!(exif.mime_type(), "");
}

#[test]
fn entry_value_to_epoch_datetime_aware() {
    // chrono::DateTime<FixedOffset> via parse: 2024-01-01 12:00:00+00:00 = 1704110400
    let dt = chrono::DateTime::parse_from_rfc3339("2024-01-01T12:00:00+00:00").unwrap();
    let v = nom_exif::EntryValue::DateTime(dt);
    // 带时区的 DateTime 不受 local_offset 影响
    assert_eq!(
        super::entry_value_to_epoch(&v, FixedOffset::east_opt(8 * 3600).unwrap()),
        1_704_110_400
    );
}

#[test]
fn entry_value_to_epoch_naive_datetime_uses_utc() {
    let nd = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let v = nom_exif::EntryValue::NaiveDateTime(nd);
    assert_eq!(super::entry_value_to_epoch(&v, utc()), 1_704_110_400);
}

/// `NaiveDateTime` 用 +08:00 解释：本地 12:00 = UTC 04:00，epoch 比 UTC 解释小 8h。
#[test]
fn entry_value_to_epoch_naive_datetime_uses_local_offset() {
    let nd = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let v = nom_exif::EntryValue::NaiveDateTime(nd);
    let offset = FixedOffset::east_opt(8 * 3600).unwrap();
    // 12:00 +08:00 = 04:00 UTC = 1_704_110_400 - 28_800
    assert_eq!(
        super::entry_value_to_epoch(&v, offset),
        1_704_110_400 - 8 * 3600
    );
}

#[test]
fn entry_value_to_epoch_non_date_variant_returns_zero() {
    let v = nom_exif::EntryValue::Text("hello".into());
    assert_eq!(super::entry_value_to_epoch(&v, utc()), 0);
}

#[test]
fn entry_value_to_epoch_negative_clamps_to_zero() {
    // 1969-12-31 23:59:59 UTC → timestamp = -1
    let dt = chrono::DateTime::parse_from_rfc3339("1969-12-31T23:59:59+00:00").unwrap();
    let v = nom_exif::EntryValue::DateTime(dt);
    assert_eq!(super::entry_value_to_epoch(&v, utc()), 0);
}

#[test]
fn populate_image_dates_on_non_image_returns_early() {
    // data_small 是非图片二进制 → MediaSource::seekable 探测 mime 应失败 → 提前 return
    let mut exif = mk_exif("image/png", |_| {});
    let reader: Box<dyn super::MediaReader> =
        Box::new(std::fs::File::open(common::DATA_SMALL).unwrap());
    super::populate_image_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original, 0);
}

#[test]
fn populate_video_dates_on_non_video_returns_early() {
    let mut exif = mk_exif("video/mp4", |_| {});
    let reader: Box<dyn super::MediaReader> =
        Box::new(std::fs::File::open(common::DATA_SMALL).unwrap());
    super::populate_video_dates(reader, &mut exif, utc());
    assert_eq!(exif.qt_create_date, 0);
}
