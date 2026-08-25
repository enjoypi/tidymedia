use std::io;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use camino::Utf8Path;
use chrono::FixedOffset;

use super::super::test_common as common;
use super::Exif;
use super::MediaReader;
use super::populate_rw2_dates;
use super::tests_common::mk_exif;
use super::tests_common::utc;

/// RW2 fixture：DTO/CreateDate/ModifyDate/Make/Model 全部命中。
#[test]
fn from_path_reads_rw2_exif() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_RW2)).unwrap();
    assert_eq!(exif.mime_type(), "image/x-panasonic-rw2");
    assert!(exif.is_media());
    // EXIF: 2017-02-14 10:30:00 UTC（fixture 以 UTC 入口解析，naive 当 UTC）
    // = 1487068200
    assert_eq!(exif.date_time_original(), 1_487_068_200);
    assert_eq!(exif.exif_create_date(), 1_487_068_201);
    assert_eq!(exif.exif_modify_date(), 1_487_068_202);
    assert_eq!(exif.make(), Some("Canon"));
    assert_eq!(exif.model(), Some("EOS 7D"));
}

/// 同一 RW2 走 +08:00 时区 → naive 按 +08:00 解释，epoch 早 8h。
#[test]
fn from_path_rw2_with_offset_applies_local_timezone() {
    let exif = Exif::from_path_with_offset(
        Utf8Path::new(common::DATA_RW2),
        FixedOffset::east_opt(8 * 3600).unwrap(),
    )
    .unwrap();
    assert_eq!(exif.date_time_original(), 1_487_068_200 - 8 * 3600);
}

/// `populate_rw2_dates` 入口 reader read 返 Err → `read_fill` `unwrap_or(0)` →
/// head 空 → `parse_rw2_exif` None → XMP fallback（无 packet）→ 字段保持 0。
/// 覆盖 `image_rw2.rs` `read_fill` Err arm。
#[test]
fn populate_rw2_dates_read_error_falls_back_to_xmp() {
    #[derive(Debug)]
    struct ErrReader;
    impl Read for ErrReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read refused"))
        }
    }
    impl Seek for ErrReader {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Ok(0)
        }
    }
    let mut exif = mk_exif("image/x-panasonic-rw2", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(ErrReader);
    populate_rw2_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// `populate_rw2_dates` 入口 reader seek(0) 失败 → 直接走 XMP fallback；
/// head 已 buffer 但无 XMP packet → 字段保持 0。覆盖 `image_rw2.rs` seek Err 分支。
#[test]
fn populate_rw2_dates_seek_failure_falls_back_to_xmp() {
    #[derive(Debug)]
    struct FailSeek(Cursor<Vec<u8>>);
    impl Read for FailSeek {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.0.read(b)
        }
    }
    impl Seek for FailSeek {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::other("seek refused"))
        }
    }
    let mut exif = mk_exif("image/x-panasonic-rw2", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(FailSeek(Cursor::new(b"II".to_vec())));
    populate_rw2_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// RW2 magic 命中但 IFD0 仅有 Make 无日期 → 双 0 触发 XMP fallback。
/// 覆盖 `image_rw2.rs` XMP fallback 单点。
#[test]
fn populate_rw2_dates_exif_no_dates_triggers_xmp_fallback() {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x0055_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    tiff.extend_from_slice(&0x010f_u16.to_le_bytes()); // Make
    tiff.extend_from_slice(&2_u16.to_le_bytes());
    tiff.extend_from_slice(&5_u32.to_le_bytes());
    tiff.extend_from_slice(&26_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff.extend_from_slice(b"Cam\0\0");

    let mut exif = mk_exif("image/x-panasonic-rw2", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(tiff));
    populate_rw2_dates(reader, &mut exif, utc());
    assert_eq!(exif.make(), Some("Cam"));
    assert_eq!(exif.date_time_original(), 0); // 触发 XMP fallback 但无 packet
    assert_eq!(exif.exif_create_date(), 0);
}

/// RW2 含 CreateDate（非 0）但无 DTO → 短路 `&&` 右侧分支跳过 XMP fallback。
#[test]
fn populate_rw2_dates_short_circuits_when_only_create_date_present() {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x0055_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    tiff.extend_from_slice(&0x8769_u16.to_le_bytes()); // ExifIFDPointer
    tiff.extend_from_slice(&4_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&26_u32.to_le_bytes()); // ExifIFD @ 26
    tiff.extend_from_slice(&0_u32.to_le_bytes()); // next IFD
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // ExifIFD count=1
    tiff.extend_from_slice(&0x9004_u16.to_le_bytes()); // CreateDate
    tiff.extend_from_slice(&2_u16.to_le_bytes());
    tiff.extend_from_slice(&20_u32.to_le_bytes());
    tiff.extend_from_slice(&44_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes()); // next IFD
    tiff.extend_from_slice(b"2017:02:14 10:30:00\0");

    let mut exif = mk_exif("image/x-panasonic-rw2", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(tiff));
    populate_rw2_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 1_487_068_200);
}

/// `from_reader` 显式分流：直接以 RW2 MIME 喂 reader → RW2 分支命中。
#[test]
fn from_reader_routes_rw2_branch() {
    let bytes = std::fs::read(common::DATA_RW2).unwrap();
    let exif = Exif::from_reader(Box::new(Cursor::new(bytes)), "image/x-panasonic-rw2", utc());
    assert_eq!(exif.date_time_original(), 1_487_068_200);
    assert_eq!(exif.make(), Some("Canon"));
}
