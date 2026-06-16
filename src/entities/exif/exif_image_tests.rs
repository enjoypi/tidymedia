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

/// 合成 JPEG fixture：nom-exif `parse_exif` 因 `ExifIFD` 越界 count 整体失败，
/// 自实现 fallback（`image_jpeg::parse_jpeg_app1_exif`）仍能读出 `IFD0` Make/Model
/// 与 `ExifIFD` 第一个 entry 的 `DateTimeOriginal`。模拟 Canon EOS 7D `MakerNotes`
/// 偏移异常场景。
#[test]
fn from_path_jpeg_app1_broken_falls_back_to_self_parse() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_APP1_BROKEN)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    // 2017-02-14 10:30:00 UTC = 1487068200
    assert_eq!(exif.date_time_original(), 1_487_068_200);
    assert_eq!(exif.make(), Some("Cam"));
    assert_eq!(exif.model(), Some("Model"));
}

/// 截断 JPEG：nom-exif `parse_exif` Err + `parse_jpeg_app1_exif` 也找不到 Exif APP1
/// → fallback 链最后退到 XMP fallback（无 packet）→ 字段全 0。覆盖 image.rs L50 None 分支。
#[test]
fn populate_image_dates_jpeg_truncated_falls_through_to_xmp() {
    use std::io::Cursor;
    // SOI + COM segment(len=2 = 仅 length 字段) + EOF：nom-exif parse_exif 必 Err
    let buf = vec![0xFF_u8, 0xD8, 0xFF, 0xFE, 0x00, 0x02];
    let mut exif = mk_exif("image/jpeg", |_| {});
    let reader: Box<dyn super::MediaReader> = Box::new(Cursor::new(buf));
    super::populate_image_dates(reader, &mut exif, super::tests_common::utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.make(), None);
}

/// `apply_tiff_ifd` 喂空 `TiffIfd::default()` → 三 `if let Some` 全 Else 分支，
/// 字段保持初值 0。覆盖 image.rs L89 BRDA Else 分支。
#[test]
fn apply_tiff_ifd_with_empty_fields_writes_nothing() {
    use super::super::tiff_ifd::TiffIfd;
    let mut exif = mk_exif("image/jpeg", |e| {
        e.make = Some("preset".to_string());
    });
    super::apply_tiff_ifd(&mut exif, TiffIfd::default(), super::tests_common::utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.exif_modify_date(), 0);
    assert_eq!(exif.make(), None); // 被 None 覆盖
    assert_eq!(exif.model(), None);
}

/// 验证 fixture 的 nom-exif 主路径**真的**失败（fixture 失效时立刻报警）。
/// 这是 fallback 路径覆盖率的前提；fixture 改动若让 nom-exif 突然接受，
/// 上面 fallback 集成 case 仍可能通过（走主路径），但本 case 会失败提示。
#[test]
fn jpeg_app1_broken_fixture_is_rejected_by_nom_exif() {
    use std::io::Cursor;
    let data = std::fs::read(common::DATA_JPEG_APP1_BROKEN).unwrap();
    let ms = nom_exif::MediaSource::seekable(Cursor::new(data)).unwrap();
    let mut parser = nom_exif::MediaParser::new();
    let result = parser.parse_exif(ms);
    assert!(
        result.is_err()
            || result.is_ok_and(|iter| {
                let parsed: nom_exif::Exif = iter.into();
                // 即便 Ok，也是空 EXIF（双 0），fallback 仍需触发
                parsed.get(nom_exif::ExifTag::DateTimeOriginal).is_none()
                    && parsed.get(nom_exif::ExifTag::Make).is_none()
            }),
        "fixture must trigger fallback path (either parse_exif Err or empty EXIF)"
    );
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
