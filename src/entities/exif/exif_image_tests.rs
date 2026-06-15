use camino::Utf8Path;
use chrono::FixedOffset;

use super::super::test_common as common;
use super::Exif;
use super::tests_common::mk_exif;

#[test]
fn from_path_reads_jpeg_with_exif() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert!(exif.is_media());
    // EXIF: DateTimeOriginal=2024-01-01, CreateDate=2024-01-02 (UTC).
    // ModifyDate=2024-01-03 读取但不进时间候选，仅供多数派仲裁识别 re-save。
    assert_eq!(exif.date_time_original(), 1_704_110_400);
    assert_eq!(exif.exif_create_date(), 1_704_196_800);
    assert_eq!(exif.exif_modify_date(), 1_704_283_200);
}

/// 同一张 JPEG，传入 +08:00 时区 → `NaiveDateTime` 按 +08:00 解释，epoch 早 8h。
#[test]
fn from_path_with_offset_applies_local_timezone() {
    let exif = Exif::from_path_with_offset(
        Utf8Path::new(common::DATA_JPEG_WITH_EXIF),
        FixedOffset::east_opt(8 * 3600).unwrap(),
    )
    .unwrap();
    assert_eq!(exif.date_time_original(), 1_704_110_400 - 8 * 3600);
}

#[test]
fn from_path_reads_mp4_with_track() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MP4_WITH_TRACK)).unwrap();
    assert!(exif.mime_type().starts_with("video/"));
    assert!(exif.is_media());
    // ffmpeg 注入 creation_time=2024-01-04T12:00:00Z
    assert_eq!(exif.qt_create_date(), 1_704_369_600);
}

/// JPEG 含 EXIF block 但无 date 标签 → 两个 if let Some 都走 None 分支。
#[test]
fn from_path_reads_jpeg_with_exif_but_no_dates() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_NO_DATES)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// MKV 有 track 但无 `CreateDate` → `populate_video_dates` 的 if let None 分支。
#[test]
fn from_path_reads_mkv_without_track_date() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MKV_NO_TRACK_DATE)).unwrap();
    assert!(exif.mime_type().starts_with("video/"));
    assert_eq!(exif.qt_create_date(), 0);
}

/// MKV 含 DateUTC=2023-06-15T10:30:00Z → `qt_create_date` 解析到正确 epoch，
/// `is_mkv_container()` 返回 true（MkvDateUtc 分流）。
#[test]
fn from_path_reads_mkv_with_date_utc() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MKV_WITH_DATE)).unwrap();
    assert!(exif.mime_type().starts_with("video/"));
    assert!(
        exif.is_mkv_container(),
        "MKV fixture MIME should be x-matroska or webm"
    );
    // 2023-06-15T10:30:00Z = 1686825000
    assert_eq!(
        exif.qt_create_date(),
        1_686_825_000,
        "MKV DateUTC should parse to 2023-06-15T10:30:00Z"
    );
}

/// `is_mkv_container()` 对 MP4 返回 false。
#[test]
fn is_mkv_container_false_for_mp4() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_MP4_WITH_TRACK)).unwrap();
    assert!(!exif.is_mkv_container());
}

/// `is_mkv_container()` 对 `video/webm` 返回 true（`||` 右侧分支）。
#[test]
fn is_mkv_container_true_for_webm_mime() {
    // 直接通过 mk_exif 构造带 webm MIME 的 Exif，无需真实 WebM 文件。
    let exif = mk_exif("video/webm", |_| {});
    assert!(exif.is_mkv_container());
}

/// JPEG 同时含 `Make` 和 `Model` EXIF 标签 → `make()` / `model()` 各返回 Some。
#[test]
fn from_path_reads_jpeg_with_make_and_model() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_MAKE_MODEL)).unwrap();
    assert_eq!(exif.make(), Some("TestCam"), "Make should be TestCam");
    assert_eq!(exif.model(), Some("TestModel"), "Model should be TestModel");
}

/// JPEG 无 `Model` 标签 → `model()` 返回 None。
#[test]
fn from_path_jpeg_without_model_returns_none() {
    // sample-with-exif.jpg 只有日期无 Make/Model → model() = None
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert!(exif.model().is_none());
    assert!(exif.make().is_none());
}

/// chmod 000 让 `infer::get_from_path` `失败（fs::metadata` 仍可工作），
/// 覆盖 `from_path` 第二个 `?` Err 分支。
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
