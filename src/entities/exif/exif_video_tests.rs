use camino::Utf8Path;
use chrono::FixedOffset;

use super::super::test_common as common;
use super::Exif;
use super::tests_common::utc;

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
fn from_path_reads_canon_m2ts_mdpm_date() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_M2TS_CANON)).unwrap();
    assert_eq!(exif.mime_type(), "video/m2ts");
    assert!(exif.is_media());
    // fixture MDPM "2011:10:01 10:35:57"，按 UTC 解释
    let want = chrono::NaiveDate::from_ymd_opt(2011, 10, 1)
        .unwrap()
        .and_hms_opt(10, 35, 57)
        .unwrap()
        .and_utc()
        .timestamp()
        .cast_unsigned();
    assert_eq!(exif.date_time_original(), want);
    // MDPM 是单一拍摄时刻：不伪造 P1 / 容器时间
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.qt_create_date(), 0);
}

#[test]
fn m2ts_offset_shifts_epoch() {
    // 同一 MDPM naive 时间按 +8 解释应比 UTC 提前 8 小时。
    let east8 = FixedOffset::east_opt(8 * 3600).unwrap();
    let f = || std::fs::File::open(common::DATA_M2TS_CANON).unwrap();
    let utc_exif = Exif::from_reader(Box::new(f()), "video/m2ts", utc());
    let east_exif = Exif::from_reader(Box::new(f()), "video/m2ts", east8);
    assert_eq!(
        utc_exif.date_time_original() - east_exif.date_time_original(),
        8 * 3600
    );
}

#[test]
fn from_reader_m2ts_without_mdpm_leaves_fields_zero() {
    // BDAV sync pattern 通过嗅探但无 MDPM SEI：let-else 早返回。
    let mut bytes = vec![0u8; 384];
    bytes[4] = 0x47;
    bytes[196] = 0x47;
    let reader: Box<dyn super::MediaReader> = Box::new(std::io::Cursor::new(bytes));
    let exif = Exif::from_reader(reader, "video/m2ts", utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
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
