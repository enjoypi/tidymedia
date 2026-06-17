//! CFB 字节扫描单测：合成 `PropertySet` 字节缓冲覆盖 `extract_dates` /
//! `find_property_filetime` / `filetime_to_epoch` / `read_u32_le` 各分支。
//! 整 fn `parse(reader, mime)` `coverage(off)`，e2e 由 fixture 集成测试覆盖
//! （但 CFB 写 lib 罕见 → 不入 subprocess fixture，业务由本节单测真测）。

use super::*;

/// 构造合法 `SummaryInformation PropertySet` 字节缓冲，含 `PID_CREATE_DTM` +
/// `PID_LASTSAVE_DTM` 两 FILETIME 属性。仅测试 + 内部 helper 使用。
fn build_summary_propertyset(created_ft: u64, modified_ft: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF]); // ByteOrder LE
    buf.extend_from_slice(&[0, 0]); // Version
    buf.extend_from_slice(&[0, 0, 0, 0]); // SystemId
    buf.extend_from_slice(&[0u8; 16]); // CLSID
    buf.extend_from_slice(&1u32.to_le_bytes()); // NumPropertySets
    buf.extend_from_slice(&FORMAT_ID_SUMMARY); // FMTID
    buf.extend_from_slice(&48u32.to_le_bytes()); // section offset (after header)

    let mut section = Vec::new();
    section.extend_from_slice(&0u32.to_le_bytes()); // section size placeholder
    section.extend_from_slice(&2u32.to_le_bytes()); // num properties
    // entries (2 * 8 bytes)
    section.extend_from_slice(&PID_CREATE_DTM.to_le_bytes());
    section.extend_from_slice(&24u32.to_le_bytes()); // prop 1 at offset 24
    section.extend_from_slice(&PID_LASTSAVE_DTM.to_le_bytes());
    section.extend_from_slice(&36u32.to_le_bytes()); // prop 2 at offset 36
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&created_ft.to_le_bytes());
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&modified_ft.to_le_bytes());

    #[expect(
        clippy::cast_possible_truncation,
        reason = "section 长度 < 256，u32 cast 安全"
    )]
    let section_size = section.len() as u32;
    section[0..4].copy_from_slice(&section_size.to_le_bytes());

    buf.extend(section);
    buf
}

/// Unix epoch 秒数  → FILETIME 100ns ticks。
fn unix_to_filetime(unix_secs: u64) -> u64 {
    (unix_secs + EPOCH_DELTA_SECS) * FILETIME_TICKS_PER_SEC
}

// ============= extract_dates 主路径 =============

#[test]
fn extract_dates_happy_path() {
    // 2017-02-14 10:30:00 UTC = 1_487_068_200
    // 2018-01-01 12:00:00 UTC = 1_514_808_000
    let buf = build_summary_propertyset(
        unix_to_filetime(1_487_068_200),
        unix_to_filetime(1_514_808_000),
    );
    assert_eq!(extract_dates(&buf), (1_487_068_200, 1_514_808_000));
}

#[test]
fn extract_dates_too_short_buf_returns_zeros() {
    assert_eq!(extract_dates(b""), (0, 0));
}

// ============= find_property_filetime 边界 =============

#[test]
fn find_property_filetime_pid_not_present() {
    let buf = build_summary_propertyset(0, 0);
    // PID 0xFFFF 不存在 → None。
    assert!(find_property_filetime(&buf, 0xFFFF).is_none());
}

#[test]
fn find_property_filetime_short_buf_returns_none() {
    assert!(find_property_filetime(&[0u8; 10], PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_byte_order_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[0] = 0x00; // 破坏 ByteOrder
    buf[1] = 0x00;
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_fmtid_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[28] = 0xFF; // 破坏 FMTID
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_section_offset_out_of_range_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[44..48].copy_from_slice(&u32::MAX.to_le_bytes()); // section_off 越界
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_section_too_short_returns_none() {
    // section_off=48 让 section = buf[48..] 只剩 4 字节 < 8 → 触发 short section 早返。
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF, 0, 0, 0, 0, 0, 0]);
    buf.extend_from_slice(&[0u8; 16]);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&FORMAT_ID_SUMMARY);
    buf.extend_from_slice(&48u32.to_le_bytes()); // section_off=48 = buf 末尾起点
    // section starts at buf[48..]; total buf = 52 → section.len() = 4 < 8。
    buf.extend_from_slice(&[0u8; 4]);
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_num_props_too_large_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    // section 在 offset 48，section's num_props at offset 48+4=52
    buf[52..56].copy_from_slice(&500u32.to_le_bytes()); // > 256 → None
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_entries_truncated_returns_none() {
    // section 仅 8 字节 header（声明 num_props=5）但无 entries 数据 → entries 越界。
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF, 0, 0, 0, 0, 0, 0]);
    buf.extend_from_slice(&[0u8; 16]);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&FORMAT_ID_SUMMARY);
    buf.extend_from_slice(&48u32.to_le_bytes()); // section_off=48
    // section header: size=8, num_props=5 → entries_end=48 > section.len()=8
    buf.extend_from_slice(&8u32.to_le_bytes()); // size
    buf.extend_from_slice(&5u32.to_le_bytes()); // num
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_property_offset_out_of_range_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    // 让 PID_CREATE_DTM 的 offset = u32::MAX (在 section 内 offset 8+4=12)
    let entry_off_in_buf = 48 + 8 + 4; // section_off + header + first entry id len
    buf[entry_off_in_buf..entry_off_in_buf + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_vt_type_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    // 把 property 1 的 VT_FILETIME (offset 48 + 24 = 72) 改成其他类型
    buf[72..76].copy_from_slice(&0x1Eu32.to_le_bytes()); // VT_LPSTR
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

// ============= filetime_to_epoch 边界 =============

#[test]
fn filetime_to_epoch_modern_date() {
    let ticks = unix_to_filetime(1_487_068_200);
    assert_eq!(filetime_to_epoch(ticks), Some(1_487_068_200));
}

#[test]
fn filetime_to_epoch_zero_returns_none() {
    // ticks=0 → secs=0 < EPOCH_DELTA → None
    assert!(filetime_to_epoch(0).is_none());
}

#[test]
fn filetime_to_epoch_below_unix_epoch_returns_none() {
    // 1969 年的 FILETIME，secs < EPOCH_DELTA → None
    assert!(filetime_to_epoch(EPOCH_DELTA_SECS * FILETIME_TICKS_PER_SEC).is_none());
}

// ============= u32_le_at =============

#[test]
fn u32_le_at_happy() {
    assert_eq!(u32_le_at(&[1, 0, 0, 0], 0), 1);
}
