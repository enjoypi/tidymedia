use super::parse_rw2_exif;

fn u16le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn ifd_entry(tag: u16, typ: u16, cnt: u32, val: u32) -> Vec<u8> {
    let mut e = Vec::with_capacity(12);
    e.extend_from_slice(&u16le(tag));
    e.extend_from_slice(&u16le(typ));
    e.extend_from_slice(&u32le(cnt));
    e.extend_from_slice(&u32le(val));
    e
}

/// 完整 RW2：IFD0 含 Make+ExifIFDPointer，ExifIFD 含 DTO+CreateDate+ModifyDate。
fn build_rw2_full() -> Vec<u8> {
    // 布局（与 gen_rw2.ts 同构）：
    //   0..8   : header (II + 0x0055 + IFD0 offset=8)
    //   8..10  : IFD0 count=2 → Make + ExifIFDPointer
    //   10..34 : 2 entries × 12
    //   34..38 : IFD0 next=0
    //   38..40 : ExifIFD count=3 → DTO/CreateDate/ModifyDate
    //   40..76 : 3 entries × 12
    //   76..80 : ExifIFD next=0
    //   80..100: DTO / 100..120: CreateDate / 120..140: ModifyDate / 140..146: Make
    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&u16le(0x0055));
    buf.extend_from_slice(&u32le(8));
    buf.extend_from_slice(&u16le(2));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 140));
    buf.extend_from_slice(&ifd_entry(0x8769, 4, 1, 38));
    buf.extend_from_slice(&u32le(0));
    buf.extend_from_slice(&u16le(3));
    buf.extend_from_slice(&ifd_entry(0x9003, 2, 20, 80));
    buf.extend_from_slice(&ifd_entry(0x9004, 2, 20, 100));
    buf.extend_from_slice(&ifd_entry(0x0132, 2, 20, 120));
    buf.extend_from_slice(&u32le(0));
    buf.extend_from_slice(b"2024:05:17 12:00:00\0");
    buf.extend_from_slice(b"2024:05:17 12:00:01\0");
    buf.extend_from_slice(b"2024:05:17 12:00:02\0");
    buf.extend_from_slice(b"Canon\0");
    buf
}

#[test]
fn parse_rw2_exif_le_extracts_fields() {
    let ifd = parse_rw2_exif(&build_rw2_full()).unwrap();
    assert_eq!(
        ifd.date_time_original.as_deref(),
        Some("2024:05:17 12:00:00")
    );
    assert_eq!(ifd.create_date.as_deref(), Some("2024:05:17 12:00:01"));
    assert_eq!(ifd.modify_date.as_deref(), Some("2024:05:17 12:00:02"));
    assert_eq!(ifd.make.as_deref(), Some("Canon"));
    assert_eq!(ifd.model, None);
}

#[test]
fn parse_rw2_exif_tiff_magic_returns_none() {
    let mut buf = build_rw2_full();
    buf[2] = 0x2a;
    buf[3] = 0x00;
    assert!(parse_rw2_exif(&buf).is_none());
}

#[test]
fn parse_rw2_exif_short_buffer_returns_none() {
    assert!(parse_rw2_exif(b"II").is_none());
}

#[test]
fn parse_rw2_exif_bad_bom_returns_none() {
    let mut buf = build_rw2_full();
    buf[0] = b'X';
    buf[1] = b'X';
    assert!(parse_rw2_exif(&buf).is_none());
}

#[test]
fn parse_rw2_exif_be_magic_rejected() {
    // RW2 只有 LE 变体（Panasonic 固定 II）；BE magic 等价字节走同解析但
    // 0x0055 BE 布局读到的 IFD0 offset 越界 → None。此处仅验证非 II BOM 拒绝。
    let mut buf = build_rw2_full();
    buf[0] = b'M';
    buf[1] = b'M';
    assert!(parse_rw2_exif(&buf).is_none());
}
