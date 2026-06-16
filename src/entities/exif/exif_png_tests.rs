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
use super::populate_png_dates;
use super::tests_common::mk_exif;
use super::tests_common::utc;

/// PNG 含 `eXIf` chunk：DTO/CreateDate/ModifyDate/Make/Model 全部命中。
#[test]
fn from_path_reads_png_with_exif_chunk() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_PNG_EXIF)).unwrap();
    assert_eq!(exif.mime_type(), "image/png");
    assert!(exif.is_media());
    // EXIF: 2017-02-14 10:30:00 UTC（fixture 以 UTC 入口解析，naive 当 UTC）
    // = 1487068200
    assert_eq!(exif.date_time_original(), 1_487_068_200);
    assert_eq!(exif.exif_create_date(), 1_487_068_201);
    assert_eq!(exif.exif_modify_date(), 1_487_068_202);
    assert_eq!(exif.make(), Some("Canon"));
    assert_eq!(exif.model(), Some("EOS 7D"));
}

/// 同一 PNG 走 +08:00 时区 → naive 按 +08:00 解释，epoch 早 8h。
#[test]
fn from_path_png_with_offset_applies_local_timezone() {
    let exif = Exif::from_path_with_offset(
        Utf8Path::new(common::DATA_PNG_EXIF),
        FixedOffset::east_opt(8 * 3600).unwrap(),
    )
    .unwrap();
    assert_eq!(exif.date_time_original(), 1_487_068_200 - 8 * 3600);
}

/// 无 `eXIf` chunk 的 PNG（`DNSBenchmark.png`）：PNG 自解析路径返 None →
/// XMP fallback 也无 packet → 三日期字段全归零。验证既有 PNG 路径未被破坏。
#[test]
fn from_path_png_without_exif_chunk_returns_zero() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_DNS_BENCHMARK)).unwrap();
    assert_eq!(exif.mime_type(), "image/png");
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
    assert_eq!(exif.exif_modify_date(), 0);
    assert_eq!(exif.make(), None);
    assert_eq!(exif.model(), None);
}

/// `populate_png_dates` 入口 reader seek(0) 失败 → 直接走 XMP fallback；
/// head 已 buffer 但无 XMP packet → 字段保持 0。覆盖 `image_png.rs` L30 分支。
#[test]
fn populate_png_dates_seek_failure_falls_back_to_xmp() {
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
    let mut exif = mk_exif("image/png", |_| {});
    let reader: Box<dyn MediaReader> =
        Box::new(FailSeek(Cursor::new(b"\x89PNG\r\n\x1a\n".to_vec())));
    populate_png_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 0);
}

/// PNG `eXIf` chunk 命中但 IFD0 仅有 Make 无日期 → 双 0 触发 XMP fallback。
/// 覆盖 `image_png.rs` L42 分支。
#[test]
fn populate_png_dates_exif_chunk_with_no_dates_triggers_xmp_fallback() {
    // 手写 PNG：sig + IHDR + eXIf (TIFF with only Make=Cam) + IEND，无 XMP packet
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x002A_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    tiff.extend_from_slice(&0x010f_u16.to_le_bytes()); // Make
    tiff.extend_from_slice(&2_u16.to_le_bytes());
    tiff.extend_from_slice(&5_u32.to_le_bytes());
    tiff.extend_from_slice(&26_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff.extend_from_slice(b"Cam\0\0");

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    // IHDR chunk: len=13 + type + 13 bytes + 4 CRC（不验证）
    png.extend_from_slice(&13_u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&[0u8; 13]);
    png.extend_from_slice(&[0u8; 4]);
    // eXIf chunk
    png.extend_from_slice(&u32::try_from(tiff.len()).unwrap().to_be_bytes());
    png.extend_from_slice(b"eXIf");
    png.extend_from_slice(&tiff);
    png.extend_from_slice(&[0u8; 4]);
    // IEND
    png.extend_from_slice(&[0u8; 4]);
    png.extend_from_slice(b"IEND");
    png.extend_from_slice(&[0u8; 4]);

    let mut exif = mk_exif("image/png", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(png));
    populate_png_dates(reader, &mut exif, utc());
    assert_eq!(exif.make(), Some("Cam"));
    assert_eq!(exif.date_time_original(), 0); // 仍 0：触发 XMP fallback 但无 packet
    assert_eq!(exif.exif_create_date(), 0);
}

/// PNG `eXIf` 含 CreateDate（非 0）但无 DTO → 短路 `&&` 右侧分支跳过
/// XMP fallback；覆盖 `image_png.rs` L42 BRDA 短路右侧。
#[test]
fn populate_png_dates_short_circuits_when_only_create_date_present() {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x002A_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    // IFD0: ExifIFDPointer → ExifIFD
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    tiff.extend_from_slice(&0x8769_u16.to_le_bytes()); // ExifIFDPointer
    tiff.extend_from_slice(&4_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&26_u32.to_le_bytes()); // ExifIFD @ 26
    tiff.extend_from_slice(&0_u32.to_le_bytes()); // next IFD
    // ExifIFD @ 26：仅 CreateDate
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // count=1
    tiff.extend_from_slice(&0x9004_u16.to_le_bytes()); // CreateDate
    tiff.extend_from_slice(&2_u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&20_u32.to_le_bytes()); // cnt
    tiff.extend_from_slice(&44_u32.to_le_bytes()); // offset 44（ExifIFD 完后 cum=44）
    tiff.extend_from_slice(&0_u32.to_le_bytes()); // next IFD
    tiff.extend_from_slice(b"2017:02:14 10:30:00\0"); // 44..64

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png.extend_from_slice(&13_u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&[0u8; 13]);
    png.extend_from_slice(&[0u8; 4]);
    png.extend_from_slice(&u32::try_from(tiff.len()).unwrap().to_be_bytes());
    png.extend_from_slice(b"eXIf");
    png.extend_from_slice(&tiff);
    png.extend_from_slice(&[0u8; 4]);
    png.extend_from_slice(&[0u8; 4]);
    png.extend_from_slice(b"IEND");
    png.extend_from_slice(&[0u8; 4]);

    let mut exif = mk_exif("image/png", |_| {});
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(png));
    populate_png_dates(reader, &mut exif, utc());
    assert_eq!(exif.date_time_original(), 0);
    assert_eq!(exif.exif_create_date(), 1_487_068_200);
}
