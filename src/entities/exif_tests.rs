use camino::Utf8Path;
use chrono::FixedOffset;

use super::super::test_common as common;
use super::Exif;

fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

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

#[test]
fn from_path_reads_jpeg_with_exif() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert!(exif.is_media());
    // EXIF: DateTimeOriginal=2024-01-01, CreateDate=2024-01-02 (UTC).
    // ModifyDate=2024-01-03 在 EXIF block 里存在但故意不读取（避免编辑时间污染）。
    assert_eq!(exif.date_time_original(), 1_704_110_400);
    assert_eq!(exif.exif_create_date(), 1_704_196_800);
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

/// JPEG 含 GPS 时间字段 → `gps_utc()` 解析到 2023-06-15T10:30:00Z。
#[test]
fn from_path_reads_jpeg_with_gps_utc() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_GPS)).unwrap();
    let gps = exif.gps_utc().expect("GPS fixture must have gps_utc");
    // 2023-06-15T10:30:00Z = 1686825000
    assert_eq!(
        gps.timestamp(),
        1_686_825_000,
        "GPS UTC should be 2023-06-15T10:30:00Z"
    );
}

/// JPEG 无 GPS 字段 → `gps_utc()` 返回 None。
#[test]
fn from_path_jpeg_without_gps_returns_none() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert!(
        exif.gps_utc().is_none(),
        "fixture without GPS must return None"
    );
}

/// `parse_gps_date`：date 格式非法 → None。
#[test]
fn parse_gps_date_invalid_returns_none() {
    assert!(super::parse_gps_date("not-a-date").is_none());
    assert!(super::parse_gps_date("2023:13:01").is_none()); // month 13 invalid
    // 数字合法但段数不足：必须由 len 前置 guard 拦下（而非依赖 parse 失败或越界 panic）
    assert!(super::parse_gps_date("2024:05").is_none());
}

/// `rational_to_u32`：denominator=0 → None。
#[test]
fn rational_to_u32_zero_denominator_returns_none() {
    let r = nom_exif::URational::new(10, 0);
    assert!(super::rational_to_u32(r).is_none());
}

/// `rational_to_u32`：denominator>1 时做真除法（10/2=5，杀「/ 变 *」类算术变异；
/// 既有用例 denom 全是 0 或 1，1 时乘除等价）。
#[test]
fn rational_to_u32_divides_by_denominator() {
    let r = nom_exif::URational::new(10, 2);
    assert_eq!(super::rational_to_u32(r), Some(5));
}

/// `build_gps_utc`：date 或 time 任一为 None → None。
#[test]
fn build_gps_utc_missing_date_returns_none() {
    assert!(super::build_gps_utc(None, None).is_none());
    let r = nom_exif::URational::new(10, 1);
    assert!(super::build_gps_utc(None, Some([r, r, r])).is_none());
    assert!(super::build_gps_utc(Some("2023:06:15"), None).is_none());
}

/// `build_gps_utc`：`parse_gps_date` 返 None（非法日期格式）→ `?` Err 分支。
#[test]
fn build_gps_utc_invalid_date_string_returns_none() {
    // "not-a-date" → parse_gps_date returns None → build_gps_utc returns None
    let r = nom_exif::URational::new(10, 1);
    assert!(super::build_gps_utc(Some("not-a-date"), Some([r, r, r])).is_none());
}

/// `build_gps_utc`：`rational_to_u32(h)` 返 None（分母为 0）→ `?` Err 分支。
#[test]
fn build_gps_utc_zero_denominator_rational_returns_none() {
    // zero-denominator → rational_to_u32 returns None → build_gps_utc returns None
    let zero = nom_exif::URational::new(10, 0);
    let ok = nom_exif::URational::new(10, 1);
    // h 分母为 0
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([zero, ok, ok])).is_none());
    // m 分母为 0（h 先成功，m 失败）
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([ok, zero, ok])).is_none());
    // s 分母为 0（h、m 先成功，s 失败）
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([ok, ok, zero])).is_none());
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

/// `Exif::open` 内 `sniff_mime` 失败 → `?` Err 分支。FakeBackend 让 `open_read` 返回
/// 立即 read Err 的 `reader：sniff_mime` 内 `?` 把 Err 上抛到 open。
#[test]
fn open_propagates_sniff_mime_io_error() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/x.bin"));
    fake.add_file(loc.clone(), vec![0u8; 32]);
    fake.inject_reader_error(loc.clone(), std::io::ErrorKind::Interrupted);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let err = Exif::open(&loc, &backend, utc()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("IO"), "got: {msg}");
}

fn mk_exif(mime: &str, init: impl FnOnce(&mut Exif)) -> Exif {
    let mut exif = Exif {
        mime_type: mime.to_string(),
        ..Default::default()
    };
    init(&mut exif);
    exif
}

// ── AVI（RIFF strd 内嵌 EXIF）分流 ──

const DATA_FUJI_AVI: &str = "tests/data/sample-fuji-strd.avi";

#[test]
fn from_path_reads_fuji_avi_embedded_exif() {
    let exif = Exif::from_path(Utf8Path::new(DATA_FUJI_AVI)).unwrap();
    assert_eq!(exif.mime_type(), "video/x-msvideo");
    assert!(exif.is_media());
    // fixture 内嵌 EXIF "2005:04:26 20:10:00"，按 UTC 解释
    let want = chrono::NaiveDate::from_ymd_opt(2005, 4, 26)
        .unwrap()
        .and_hms_opt(20, 10, 0)
        .unwrap()
        .and_utc()
        .timestamp()
        .cast_unsigned();
    assert_eq!(exif.date_time_original(), want);
    assert_eq!(exif.exif_create_date(), want);
    assert_eq!(exif.make(), Some("FUJIFILM"));
    assert_eq!(exif.model(), Some("FinePix E550"));
    // RIFF 路径不填 qt_create_date
    assert_eq!(exif.qt_create_date(), 0);
}

#[test]
fn avi_offset_shifts_epoch() {
    // 同一 naive 时间按 +8 解释应比 UTC 提前 8 小时。
    let east8 = FixedOffset::east_opt(8 * 3600).unwrap();
    let utc_epoch = super::ascii_datetime_to_epoch("2005:04:26 20:10:00", utc());
    let east_epoch = super::ascii_datetime_to_epoch("2005:04:26 20:10:00", east8);
    assert_eq!(utc_epoch - east_epoch, 8 * 3600);
}

#[test]
fn from_reader_avi_without_strd_leaves_fields_zero() {
    // 只有 RIFF 头的"空" AVI：parse_avi_exif None → let-else 早返回。
    let bytes = b"RIFF\x04\x00\x00\x00AVI ".to_vec();
    let reader: Box<dyn super::MediaReader> = Box::new(std::io::Cursor::new(bytes));
    let exif = Exif::from_reader(reader, "video/x-msvideo", utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.make(), None);
}

#[test]
fn ascii_datetime_to_epoch_invalid_format_returns_zero() {
    assert_eq!(super::ascii_datetime_to_epoch("not a date", utc()), 0);
}

#[test]
fn ascii_datetime_to_epoch_epoch_start_returns_zero() {
    // secs == 0 命中 `<= 0` 分支：与"字段未填"同义。
    assert_eq!(
        super::ascii_datetime_to_epoch("1970:01:01 00:00:00", utc()),
        0
    );
}

// 老 QuickTime `pnot` preview atom 起头的 MOV 文件：infer crate 只认 `ftyp`，
// 必须靠 fallback 兜底返回 `video/quicktime`，否则 `is_media` 误判致整文件被 ignore。
#[test]
fn quicktime_legacy_mime_detects_pnot_atom() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"pnot");
    assert_eq!(
        super::quicktime_legacy_mime(&buf),
        Some("video/quicktime")
    );
}

#[test]
fn quicktime_legacy_mime_unknown_tag_returns_none() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"XXXX");
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}

#[test]
fn quicktime_legacy_mime_too_short_returns_none() {
    let buf = [0u8; 7];
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}
