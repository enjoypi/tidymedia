//! JPEG APP1 `Exif\0\0` 段裸 IFD fallback。
//!
//! nom-exif `parse_exif` 在 `MakerNotes` 偏移异常等 spec 边角 case 下整体返 `Err`
//! （exiftool 报 `Adjusted MakerNotes base by -126` 即此模式：Canon EOS 7D 等
//! 早期固件）。本 fallback 跳过 nom-exif，直接扫 JPEG APP1 段拿 `IFD0`/`ExifIFD`
//! 的核心字段（DTO/CreateDate/ModifyDate/Make/Model），保住 P0/P1 归档。
//!
//! 只读 `image::populate_image_dates` 已 buffer 的 64 KiB 头部，不再 IO；
//! 多 APP1 段（XMP APP1 在前 + Exif APP1 在后）按规范遍历命中 Exif APP1。

use super::super::tiff_ifd;

/// JPEG marker prefix。
const MARKER_PREFIX: u8 = 0xFF;
/// Start of Image。
const MARKER_SOI: u8 = 0xD8;
/// APP1 segment。
const MARKER_APP1: u8 = 0xE1;
/// Start of Scan：到此即进入压缩数据，APP* 全部出现在 SOS 之前。
const MARKER_SOS: u8 = 0xDA;
/// End of Image。
const MARKER_EOI: u8 = 0xD9;

/// APP1 payload 必须以此 magic 起头才是 Exif segment；XMP/Adobe APP1 不同。
const EXIF_MAGIC: &[u8; 6] = b"Exif\0\0";

/// 扫描的 marker 上限，防恶意/损坏数据死循环。
const MAX_MARKERS: usize = 64;

/// 从 JPEG 头部 buffer（推荐 ≥ 64 KiB）提取 Exif APP1 内 TIFF/IFD 字段。
/// 非 JPEG / 无 Exif APP1 / TIFF header 损坏均返 None。
pub(super) fn parse_jpeg_app1_exif(head: &[u8]) -> Option<tiff_ifd::TiffIfd> {
    // SOI = FF D8
    if head.get(0..2)? != [MARKER_PREFIX, MARKER_SOI] {
        return None;
    }
    let mut off = 2;
    for _ in 0..MAX_MARKERS {
        // marker = 0xFF + 1 字节 marker code（连续 0xFF 是 fill byte，跳过）。
        while head.get(off) == Some(&MARKER_PREFIX) && head.get(off + 1) == Some(&MARKER_PREFIX) {
            off += 1;
        }
        if head.get(off)? != &MARKER_PREFIX {
            return None;
        }
        let code = *head.get(off + 1)?;
        if code == MARKER_SOS || code == MARKER_EOI {
            return None;
        }
        // segment length = BE u16，包含自身 2 字节但不含 marker。
        let len_bytes = head.get(off + 2..off + 4)?;
        let seg_len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
        if seg_len < 2 {
            return None;
        }
        let payload_start = off + 4;
        let payload_end = off + 2 + seg_len;
        let payload = head.get(payload_start..payload_end)?;
        if code == MARKER_APP1 && payload.get(..6) == Some(EXIF_MAGIC) {
            // TIFF header 紧随 `Exif\0\0` magic 之后。
            return tiff_ifd::parse_tiff(&payload[6..]);
        }
        off = payload_end;
    }
    None
}

#[cfg(test)]
#[path = "jpeg_fallback_tests.rs"]
mod tests;
