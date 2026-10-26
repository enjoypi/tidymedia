use std::io::{Cursor, Write};

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

include!("cfb_part1_tests.rs");
include!("cfb_part2_tests.rs");
