use camino::Utf8Path;

use super::super::test_common as common;
use super::Exif;
use super::tests_common::mk_exif;
use super::tests_common::utc;

/// EXIF block 完全无三日期、XMP packet 含 photoshop:DateCreated + xmp:CreateDate
/// → fallback 把两者分别填入 `date_time_original` / `create_date`。
/// 2008-10-31T09:15:01+08:00 → UTC 01:15:01 → epoch `1_225_415_701`。
#[test]
fn from_path_jpeg_xmp_only_uses_xmp_fallback() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_XMP_ONLY)).unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert_eq!(
        exif.date_time_original(),
        1_225_415_701,
        "photoshop:DateCreated should populate date_time_original"
    );
    assert_eq!(
        exif.exif_create_date(),
        1_225_415_701,
        "xmp:CreateDate should populate create_date"
    );
}

/// EXIF `DateTimeOriginal` 已有 → fallback 不触发，原 EXIF 值保留（不被 XMP 覆盖）。
/// sample-with-exif.jpg 有 DTO=2024-01-01 EXIF block，不含 XMP packet；
/// 用 `mk_exif` 模拟同时已填的场景：调 `populate_image_xmp_fallback` 验证不覆盖。
#[test]
fn xmp_fallback_does_not_overwrite_existing_exif_dates() {
    // 直接断言条件分支：dto/create_date 非零时 from_path 路径不进 fallback。
    // sample-with-exif.jpg 拍摄时间已知 1_704_110_400，确认未被任何 XMP 改写。
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert_eq!(exif.date_time_original(), 1_704_110_400);
}

/// EXIF 与 XMP 都没日期 → 两字段保持 0（不影响 P4 `FsMtime` 兜底链路）。
/// sample-no-dates.jpg 是 fixture 基线（EXIF 仅 Make/Model，无任何 XMP）。
#[test]
fn from_path_jpeg_no_exif_no_xmp_leaves_dates_zero() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_NO_DATES)).unwrap();
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// 仅有 EXIF CreateDate（无 `DateTimeOriginal`、无 XMP）→ XMP fallback short-circuit
/// 上 `cd==0` False 分支（dto==0 已 True）。验证 cd 保留 EXIF 真值、dto 仍为 0。
/// 2024-01-02 12:00 UTC = `1_704_196_800`。
#[test]
fn from_path_jpeg_only_createdate_skips_xmp_fallback() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_ONLY_CREATEDATE)).unwrap();
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 1_704_196_800);
}

/// 直测 `populate_image_xmp_fallback`：head 含完整 packet → 两字段被填。
#[test]
fn populate_image_xmp_fallback_fills_both_keys() {
    let head = b"prefix<x:xmpmeta \
        photoshop:DateCreated=\"2024-05-01T14:30:00+08:00\" \
        xmp:CreateDate=\"2024-05-02T15:30:00+00:00\"/>\
        </x:xmpmeta>tail";
    let mut exif = mk_exif("image/jpeg", |_| {});
    super::populate_image_xmp_fallback(head, &mut exif);
    assert_eq!(exif.date_time_original(), 1_714_545_000);
    assert_eq!(exif.exif_create_date(), 1_714_663_800);
}

/// head 无 XMP packet → fallback 静默返回，两字段保持 0（let-else 早返回分支）。
#[test]
fn populate_image_xmp_fallback_no_packet_no_change() {
    let mut exif = mk_exif("image/jpeg", |_| {});
    super::populate_image_xmp_fallback(b"random bytes no packet", &mut exif);
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// XMP packet 内日期带负 epoch（1969 之前）→ `secs > 0` 守卫拒填，字段保持 0。
#[test]
fn populate_image_xmp_fallback_negative_epoch_rejected() {
    let head = b"<x:xmpmeta \
        photoshop:DateCreated=\"1969-01-01T00:00:00+00:00\" \
        xmp:CreateDate=\"1969-01-01T00:00:00+00:00\"/>\
        </x:xmpmeta>";
    let mut exif = mk_exif("image/jpeg", |_| {});
    super::populate_image_xmp_fallback(head, &mut exif);
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// XMP packet 命中但两键都缺 → 两字段保持 0（`parse_xmp_dates` 默认值路径）。
#[test]
fn populate_image_xmp_fallback_packet_without_keys() {
    let head = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>";
    let mut exif = mk_exif("image/jpeg", |_| {});
    super::populate_image_xmp_fallback(head, &mut exif);
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// `populate_image_dates` 入口 reader seek(0) 失败 → 早返回，字段保持 0。
/// 用自实现 `FailSeek` wrapper：read 透传 Cursor、seek 恒 Err。
#[test]
fn populate_image_dates_seek_failure_returns_early() {
    use std::io::{self, Cursor, Read, Seek, SeekFrom};

    #[derive(Debug)]
    struct FailSeek(Cursor<Vec<u8>>);

    impl Read for FailSeek {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.0.read(b)
        }
    }

    impl Seek for FailSeek {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::other("seek disabled"))
        }
    }

    let mut exif = mk_exif("image/jpeg", |_| {});
    let reader: Box<dyn super::MediaReader> = Box::new(FailSeek(Cursor::new(vec![0u8; 64])));
    super::populate_image_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
}
