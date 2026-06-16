use super::AviExif;
use super::IFD_BASE;
use super::parse_avif_ifd;
use super::tests_common::MAX_ASCII_BYTES;
use super::tests_common::avif_truncated_entry;
use super::tests_common::avif_with_make;

// ── parse_avif_ifd：strd 数据解析 ──

#[test]
fn strd_without_avif_magic_returns_none() {
    assert_eq!(parse_avif_ifd(b"TIFF\0\0\0\0\0\0"), None);
}

#[test]
fn strd_shorter_than_magic_returns_none() {
    assert_eq!(parse_avif_ifd(b"AV"), None);
}

#[test]
fn strd_shorter_than_ifd_base_returns_none() {
    assert_eq!(parse_avif_ifd(b"AVIF\0\0"), None);
}

#[test]
fn ifd_count_claims_more_entries_than_present_returns_none() {
    // count=3 但只有 1 个 entry 的空间 → 越界 → 整体拒绝。
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE] = 3;
    assert_eq!(parse_avif_ifd(&strd), None);
}

#[test]
fn missing_exif_offset_still_returns_ifd0_fields() {
    // 无 ExifOffset 指针：第二趟扫描空转，IFD0 的 Make 仍保留。
    let got = parse_avif_ifd(&avif_with_make(b"FUJIFILM\0")).unwrap();
    assert_eq!(got.make.as_deref(), Some("FUJIFILM"));
    assert_eq!(got.date_time_original, None);
}

#[test]
fn inline_ascii_count_le_4_is_rejected() {
    // cnt<=4 的内联 ASCII 对目标标签不应出现，按损坏数据拒绝。
    let mut strd = avif_with_make(b"FUJI\0\0\0\0\0");
    strd[IFD_BASE + 2 + 4] = 4; // entry.cnt: 9 → 4
    let got = parse_avif_ifd(&strd).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn ascii_count_over_cap_is_rejected() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    let cnt = u32::try_from(MAX_ASCII_BYTES + 1).unwrap();
    strd[IFD_BASE + 2 + 4..IFD_BASE + 2 + 8].copy_from_slice(&cnt.to_le_bytes());
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn ascii_offset_beyond_buffer_is_rejected() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2 + 8..IFD_BASE + 2 + 12].copy_from_slice(&999u32.to_le_bytes());
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn non_utf8_ascii_is_rejected() {
    let got = parse_avif_ifd(&avif_with_make(b"\xff\xfe\xfd\xfc\xfb\0")).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn blank_ascii_is_rejected() {
    let got = parse_avif_ifd(&avif_with_make(b"    \0\0\0\0\0")).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn make_with_non_ascii_type_is_ignored() {
    // (TAG_MAKE, 非 TYPE_ASCII) 不匹配任何 arm → 走 `_` 跳过。
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2 + 2] = 1; // entry.typ: 2 → 1 (BYTE)
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn entry_with_unknown_tag_is_skipped() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2] = 0xff; // tag 低字节改成未知标签
    let got = parse_avif_ifd(&strd).unwrap();
    assert_eq!(got, AviExif::default());
}

// ── scan_ifd：entry 各字段处截断 ──

#[test]
fn ifd_entry_truncated_at_typ_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(2)), None);
}

#[test]
fn ifd_entry_truncated_at_cnt_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(5)), None);
}

#[test]
fn ifd_entry_truncated_at_val_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(9)), None);
}

// strd 头部即截断（IFD count u16le 都不够读）→ scan_ifd 首句 u16le(base, 0)? 返 None。
// 这是 line 156 `let count = u16le(base, ifd_off)? as usize;` 的 ? Err arm。
#[test]
fn ifd_count_truncated_returns_none() {
    // AVIF magic(4) + reserved(4) + 1 字节（不够 u16 count）= 9 字节
    let strd = vec![b'A', b'V', b'I', b'F', 0, 0, 0, 0, 0];
    assert_eq!(parse_avif_ifd(&strd), None);
}
