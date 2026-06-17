use super::ByteOrder;
use super::TiffIfd;
use super::parse_ifds;
use super::parse_tiff;

// ---------- byte-level fixture builder ----------

fn u16_bytes(v: u16, order: ByteOrder) -> [u8; 2] {
    match order {
        ByteOrder::Le => v.to_le_bytes(),
        ByteOrder::Be => v.to_be_bytes(),
    }
}

fn u32_bytes(v: u32, order: ByteOrder) -> [u8; 4] {
    match order {
        ByteOrder::Le => v.to_le_bytes(),
        ByteOrder::Be => v.to_be_bytes(),
    }
}

fn ifd_entry(tag: u16, typ: u16, cnt: u32, val: u32, order: ByteOrder) -> Vec<u8> {
    let mut e = Vec::with_capacity(12);
    e.extend_from_slice(&u16_bytes(tag, order));
    e.extend_from_slice(&u16_bytes(typ, order));
    e.extend_from_slice(&u32_bytes(cnt, order));
    e.extend_from_slice(&u32_bytes(val, order));
    e
}

/// 构造完整 TIFF：IFD0 含 Make+ExifIFDPointer，ExifIFD 含 DTO+CreateDate+ModifyDate。
fn build_tiff_full(order: ByteOrder) -> Vec<u8> {
    // 布局（offset 全部相对 TIFF header 起点；next-IFD-offset 字段 4 字节）：
    //   0..8    : TIFF header (II/MM + magic + IFD0 offset = 8)
    //   8..10   : IFD0 count = 2
    //   10..34  : 2 entries × 12 = 24 bytes
    //               entry0 = Make(ASCII, cnt=6, offset=140)
    //               entry1 = ExifIFDPointer(LONG, cnt=1, val=38)
    //   34..38  : IFD0 next-IFD offset (=0)
    //   38..40  : ExifIFD count = 3
    //   40..76  : 3 entries × 12 = 36 bytes
    //               entry0 = DateTimeOriginal(ASCII, cnt=20, offset=80)
    //               entry1 = CreateDate(ASCII, cnt=20, offset=100)
    //               entry2 = ModifyDate(ASCII, cnt=20, offset=120)
    //   76..80  : ExifIFD next-IFD offset (=0)
    //   80..100 : DTO data
    //   100..120: CreateDate data
    //   120..140: ModifyDate data
    //   140..146: Make data "Canon\0"
    let mut buf = Vec::new();
    buf.extend_from_slice(match order {
        ByteOrder::Le => b"II",
        ByteOrder::Be => b"MM",
    });
    buf.extend_from_slice(&u16_bytes(0x002A, order));
    buf.extend_from_slice(&u32_bytes(8, order));
    // IFD0
    buf.extend_from_slice(&u16_bytes(2, order));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 140, order));
    buf.extend_from_slice(&ifd_entry(0x8769, 4, 1, 38, order));
    buf.extend_from_slice(&u32_bytes(0, order));
    // ExifIFD
    buf.extend_from_slice(&u16_bytes(3, order));
    buf.extend_from_slice(&ifd_entry(0x9003, 2, 20, 80, order));
    buf.extend_from_slice(&ifd_entry(0x9004, 2, 20, 100, order));
    buf.extend_from_slice(&ifd_entry(0x0132, 2, 20, 120, order));
    buf.extend_from_slice(&u32_bytes(0, order));
    // ASCII data
    buf.extend_from_slice(b"2024:05:17 12:00:00\0");
    buf.extend_from_slice(b"2024:05:17 12:00:01\0");
    buf.extend_from_slice(b"2024:05:17 12:00:02\0");
    buf.extend_from_slice(b"Canon\0");
    buf
}

// ---------- parse_tiff ----------

#[test]
fn parse_tiff_le_full_extracts_all_fields() {
    let buf = build_tiff_full(ByteOrder::Le);
    let ifd = parse_tiff(&buf).unwrap();
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
fn parse_tiff_be_full_extracts_all_fields() {
    let buf = build_tiff_full(ByteOrder::Be);
    let ifd = parse_tiff(&buf).unwrap();
    assert_eq!(
        ifd.date_time_original.as_deref(),
        Some("2024:05:17 12:00:00")
    );
    assert_eq!(ifd.create_date.as_deref(), Some("2024:05:17 12:00:01"));
    assert_eq!(ifd.modify_date.as_deref(), Some("2024:05:17 12:00:02"));
    assert_eq!(ifd.make.as_deref(), Some("Canon"));
}

#[test]
fn parse_tiff_rejects_short_buffer() {
    assert_eq!(parse_tiff(b"I"), None);
}

#[test]
fn parse_tiff_rejects_bad_bom() {
    let mut buf = build_tiff_full(ByteOrder::Le);
    buf[0] = b'X';
    buf[1] = b'X';
    assert_eq!(parse_tiff(&buf), None);
}

#[test]
fn parse_tiff_rejects_bad_magic() {
    let mut buf = build_tiff_full(ByteOrder::Le);
    // 把 magic 改成 0x0001（合法 II BOM 但 magic 错）
    buf[2] = 0x01;
    buf[3] = 0x00;
    assert_eq!(parse_tiff(&buf), None);
}

#[test]
fn parse_tiff_truncated_at_magic_returns_none() {
    let buf = b"II".to_vec(); // 仅 BOM 无 magic
    assert_eq!(parse_tiff(&buf), None);
}

#[test]
fn parse_tiff_truncated_at_ifd0_offset_returns_none() {
    let mut buf = b"II".to_vec();
    buf.extend_from_slice(&0x002A_u16.to_le_bytes()); // magic but no offset
    assert_eq!(parse_tiff(&buf), None);
}

#[test]
fn parse_tiff_ifd0_offset_out_of_bounds_returns_none() {
    // BOM + magic + offset=9999 → 扫 IFD count 时越界
    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&0x002A_u16.to_le_bytes());
    buf.extend_from_slice(&9999_u32.to_le_bytes());
    assert_eq!(parse_tiff(&buf), None);
}

// ---------- parse_ifds (裸 IFD，RIFF 路径复用) ----------

#[test]
fn parse_ifds_le_minimal_no_exif_subifd() {
    // 单 IFD0 含 Make，无 ExifIFDPointer
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 18, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(b"Canon\0");
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Canon"));
    assert_eq!(ifd.date_time_original, None);
}

#[test]
fn parse_ifds_count_out_of_bounds_returns_none() {
    // count 字段读不到（base 太短）
    let buf = [0u8; 1];
    assert_eq!(parse_ifds(&buf, 0, ByteOrder::Le), None);
}

#[test]
fn parse_ifds_entry_out_of_bounds_preserves_earlier_entries() {
    // count=5 但仅给 1 个 entry 的空间：scan_ifd 中段越界用 break 截断（lenient），
    // 已成功解出的字段保留。本例 entry[0] 的 val=100 越界让 read_ascii 返 None，
    // 故 make 仍为 None；关键断言：parse_ifds 整体返 Some 而非 None。
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(5, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 100, ByteOrder::Le));
    let got = parse_ifds(&buf, 0, ByteOrder::Le).expect("partial parse retained");
    assert!(got.make.is_none());
}

#[test]
fn parse_ifds_exif_subifd_offset_invalid_keeps_ifd0_fields() {
    // ExifIFDPointer 指向越界位置 → 子 IFD 扫描失败但 IFD0 字段保留
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 30, ByteOrder::Le)); // Make at 30
    buf.extend_from_slice(&ifd_entry(0x8769, 4, 1, 9999, ByteOrder::Le)); // 越界
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(b"Canon\0");
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Canon"));
    assert_eq!(ifd.date_time_original, None);
}

// ---------- read_ascii 边界 ----------

#[test]
fn parse_ifds_ascii_inline_cnt_le_4_decoded_from_val() {
    // TIFF 规范：Model ASCII cnt=4 → 数据 inline 存于 entry val 字段 4 字节。
    // 新实现按规范读 inline 数据（旧实现拒绝，让 DJI/LG/FUJI 等短 Make/Model 字段丢失）。
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(
        0x0110,
        2,
        4,
        u32::from_le_bytes(*b"abcd"),
        ByteOrder::Le,
    ));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.model.as_deref(), Some("abcd"));
}

#[test]
fn parse_ifds_ascii_oversize_returns_none() {
    // cnt > MAX_ASCII_BYTES (256) → 拒绝
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x0110, 2, 9999, 18, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(&[b'A'; 300]);
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.model, None);
}

#[test]
fn parse_ifds_ascii_offset_out_of_bounds_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x0110, 2, 10, 9999, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.model, None);
}

#[test]
fn parse_ifds_ascii_invalid_utf8_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x0110, 2, 5, 18, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]);
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.model, None);
}

#[test]
fn parse_ifds_ascii_all_null_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x0110, 2, 5, 18, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(b"\0\0\0\0\0");
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.model, None);
}

// ---------- tag/typ 不匹配 → 跳过 ----------

#[test]
fn parse_ifds_unknown_tag_ignored() {
    // tag = 0x1234（未知）不影响其他字段
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x1234, 2, 8, 30, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x010f, 2, 6, 38, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(b"unknown\0");
    buf.extend_from_slice(b"Canon\0");
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Canon"));
}

#[test]
fn parse_ifds_make_with_wrong_type_ignored() {
    // Make tag 但 typ = LONG 非 ASCII → 不读
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x010f, 4, 1, 0x1234_5678, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd, TiffIfd::default());
}

#[test]
fn parse_ifds_exif_pointer_with_wrong_type_ignored() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&ifd_entry(0x8769, 2, 4, 9999, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd, TiffIfd::default());
}

/// TIFF 规范：ASCII cnt ≤ 4 时数据 inline 存在 entry 的 val 字段（4 字节）。
/// DJI（"DJI\0" cnt=4）/ LG（"LG\0" cnt=3）等短厂商名走此形式，过去被静默拒绝。
#[test]
fn parse_ifds_inline_ascii_make_short_dji() {
    let mut buf = Vec::new();
    // 1 entry：Make ASCII cnt=4，val 字段 = b"DJI\0"
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    // 手工构造 entry：tag=0x010f Make, typ=2 ASCII, cnt=4, val 字段直接是 ASCII bytes
    buf.extend_from_slice(&u16_bytes(0x010f, ByteOrder::Le)); // tag Make
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Le)); // typ ASCII
    buf.extend_from_slice(&u32_bytes(4, ByteOrder::Le)); // cnt=4
    buf.extend_from_slice(b"DJI\0"); // val 字段 4 字节 inline ASCII
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le)); // next-IFD
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("DJI"));
}

#[test]
fn parse_ifds_inline_ascii_make_two_chars_lg() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&u16_bytes(0x010f, ByteOrder::Le));
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(3, ByteOrder::Le)); // cnt=3 (LG\0)
    // val 字段 4 字节：前 3 字节是 ASCII "LG\0"，第 4 字节填充
    buf.extend_from_slice(b"LG\0\0");
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("LG"));
}

#[test]
fn parse_ifds_inline_ascii_be_byte_order() {
    // BE 字节序下 inline 同样按文件顺序读取（4 字节直接是 ASCII）。
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Be));
    buf.extend_from_slice(&u16_bytes(0x010f, ByteOrder::Be));
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Be));
    buf.extend_from_slice(&u32_bytes(4, ByteOrder::Be));
    buf.extend_from_slice(b"FUJI"); // 4 字节 inline，BE/LE 顺序对 byte string 无影响
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Be));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Be).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("FUJI"));
}

#[test]
fn parse_ifds_ascii_cnt_zero_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_bytes(1, ByteOrder::Le));
    buf.extend_from_slice(&u16_bytes(0x010f, ByteOrder::Le));
    buf.extend_from_slice(&u16_bytes(2, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le)); // cnt=0
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    buf.extend_from_slice(&u32_bytes(0, ByteOrder::Le));
    let ifd = parse_ifds(&buf, 0, ByteOrder::Le).unwrap();
    assert_eq!(ifd.make, None);
}
