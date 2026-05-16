use camino::Utf8Path;
use rstest::rstest;

use super::super::test_common as common;
use super::Exif;

#[test]
fn from_path_reads_dns_benchmark_png() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_DNS_BENCHMARK)).unwrap();
    assert_eq!(exif.mime_type(), "image/png");
    assert!(exif.is_media());
    assert!(exif.file_modify_date() > 0);
    // PNG fixture has no EXIF chunk → cascade falls through to file timestamps
    assert!(exif.media_create_date() > 0);
}

#[test]
fn from_path_non_media_file() {
    // data_small 是随机二进制（非图片/视频），infer 应识别为非媒体
    let exif = Exif::from_path(Utf8Path::new(common::DATA_SMALL)).unwrap();
    assert!(!exif.is_media());
    assert_eq!(exif.media_create_date(), 0);
}

#[test]
fn from_path_missing_returns_err() {
    let err = Exif::from_path(Utf8Path::new("/definitely/missing/xyz.png")).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("IO"), "got: {msg}");
}

#[rstest]
#[case(mk_exif("image/png", |e| e.date_time_original = 1_700_000_000), 1_700_000_000)]
#[case(mk_exif("image/png", |e| e.h264_date_time_original = 1_700_000_001), 1_700_000_001)]
#[case(mk_exif("video/mp4", |e| e.qt_media_create_date = 1_700_000_002), 1_700_000_002)]
#[case(mk_exif("video/mp4", |e| e.qt_create_date = 1_700_000_003), 1_700_000_003)]
#[case(mk_exif("image/jpeg", |e| e.exif_create_date = 1_700_000_004), 1_700_000_004)]
#[case(mk_exif("image/jpeg", |e| e.exif_modify_date = 1_700_000_005), 1_700_000_005)]
#[case(
    mk_exif("image/png", |e| {
        e.file_create_date = 1_700_000_006;
        e.file_modify_date = 1_700_000_007;
    }),
    1_700_000_006
)]
#[case(mk_exif("image/png", |e| e.file_modify_date = 1_700_000_008), 1_700_000_008)]
#[case(mk_exif("image/png", |e| e.file_create_date = 1_700_000_009), 1_700_000_009)]
fn media_create_date_priority_cascade(#[case] exif: Exif, #[case] want: u64) {
    assert_eq!(exif.media_create_date(), want);
}

#[test]
fn media_create_date_zero_when_not_media() {
    let exif = mk_exif("application/octet-stream", |e| e.date_time_original = 1_700_000_000);
    assert_eq!(exif.media_create_date(), 0);
}

#[test]
fn media_create_date_zero_when_no_signal_present() {
    let exif = mk_exif("image/png", |_| {});
    assert_eq!(exif.media_create_date(), 0);
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
    assert_eq!(exif.file_modify_date(), 0);
    assert_eq!(exif.file_create_date(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.exif_modify_date(), 0);
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.h264_date_time_original(), 0);
    assert_eq!(exif.qt_media_create_date(), 0);
    assert_eq!(exif.qt_create_date(), 0);
    assert_eq!(exif.mime_type(), "");
}

#[test]
fn entry_value_to_epoch_datetime_aware() {
    // chrono::DateTime<FixedOffset> via parse: 2024-01-01 12:00:00+00:00 = 1704110400
    let dt = chrono::DateTime::parse_from_rfc3339("2024-01-01T12:00:00+00:00").unwrap();
    let v = nom_exif::EntryValue::DateTime(dt);
    assert_eq!(super::entry_value_to_epoch(&v), 1_704_110_400);
}

#[test]
fn entry_value_to_epoch_naive_datetime() {
    let nd = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let v = nom_exif::EntryValue::NaiveDateTime(nd);
    assert_eq!(super::entry_value_to_epoch(&v), 1_704_110_400);
}

#[test]
fn entry_value_to_epoch_non_date_variant_returns_zero() {
    let v = nom_exif::EntryValue::Text("hello".into());
    assert_eq!(super::entry_value_to_epoch(&v), 0);
}

#[test]
fn entry_value_to_epoch_negative_clamps_to_zero() {
    // 1969-12-31 23:59:59 UTC → timestamp = -1
    let dt = chrono::DateTime::parse_from_rfc3339("1969-12-31T23:59:59+00:00").unwrap();
    let v = nom_exif::EntryValue::DateTime(dt);
    assert_eq!(super::entry_value_to_epoch(&v), 0);
}

#[test]
fn system_time_to_epoch_none_returns_zero() {
    assert_eq!(super::system_time_to_epoch(None), 0);
}

#[test]
fn system_time_to_epoch_before_unix_epoch_returns_zero() {
    // SystemTime::UNIX_EPOCH - 1s 在某些平台上是 Err；用 checked_sub_signed 构造
    let before = std::time::UNIX_EPOCH
        .checked_sub(std::time::Duration::from_secs(1))
        .expect("can subtract on test platform");
    assert_eq!(super::system_time_to_epoch(Some(before)), 0);
}

#[test]
fn populate_image_dates_on_non_image_returns_early() {
    // data_small 是非图片二进制 → nom_exif::read_exif 应返回 Err → 函数早返回，字段不变
    let mut exif = mk_exif("image/png", |_| {});
    super::populate_image_dates(Utf8Path::new(common::DATA_SMALL), &mut exif);
    assert_eq!(exif.date_time_original, 0);
}

#[test]
fn populate_video_dates_on_non_video_returns_early() {
    let mut exif = mk_exif("video/mp4", |_| {});
    super::populate_video_dates(Utf8Path::new(common::DATA_SMALL), &mut exif);
    assert_eq!(exif.qt_create_date, 0);
}

#[test]
fn from_path_reads_jpeg_with_exif() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert!(exif.is_media());
    // EXIF: DateTimeOriginal=2024-01-01, CreateDate=2024-01-02, ModifyDate=2024-01-03 (UTC)
    assert_eq!(exif.date_time_original(), 1_704_110_400);
    assert_eq!(exif.exif_create_date(), 1_704_196_800);
    assert_eq!(exif.exif_modify_date(), 1_704_283_200);
    // 优先级取 DateTimeOriginal
    assert_eq!(exif.media_create_date(), 1_704_110_400);
}

#[test]
fn from_path_reads_mp4_with_track() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MP4_WITH_TRACK)).unwrap();
    assert!(exif.mime_type().starts_with("video/"));
    assert!(exif.is_media());
    // ffmpeg 注入 creation_time=2024-01-04T12:00:00Z
    assert_eq!(exif.qt_create_date(), 1_704_369_600);
    assert_eq!(exif.media_create_date(), 1_704_369_600);
}

/// JPEG 含 EXIF block 但无 date 标签 → 三个 if let Some 都走 None 分支。
#[test]
fn from_path_reads_jpeg_with_exif_but_no_dates() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_NO_DATES)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.exif_modify_date(), 0);
}

/// MKV 有 track 但无 CreateDate → populate_video_dates 的 if let None 分支。
#[test]
fn from_path_reads_mkv_without_track_date() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MKV_NO_TRACK_DATE)).unwrap();
    assert!(exif.mime_type().starts_with("video/"));
    assert_eq!(exif.qt_create_date(), 0);
}

/// chmod 000 让 infer::get_from_path 失败（fs::metadata 仍可工作），
/// 覆盖 from_path 第二个 `?` Err 分支。
#[cfg(unix)]
#[test]
fn from_path_propagates_infer_io_error_when_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.bin");
    std::fs::write(&path, b"abcdef").unwrap();
    let orig = std::fs::metadata(&path).unwrap().permissions();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let utf8 = camino::Utf8PathBuf::from_path_buf(path.clone()).unwrap();
    let err = Exif::from_path(&utf8).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("IO"), "got: {msg}");

    // 恢复权限避免 tempdir 清理失败
    std::fs::set_permissions(&path, orig).unwrap();
}

fn mk_exif(mime: &str, init: impl FnOnce(&mut Exif)) -> Exif {
    let mut exif = Exif {
        mime_type: mime.to_string(),
        ..Default::default()
    };
    init(&mut exif);
    exif
}
