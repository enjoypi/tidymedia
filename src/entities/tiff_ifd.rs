//! 公共 TIFF IFD 解析（支持 II/MM byte order）。
//!
//! 复用方：
//! - `entities::png::parse_png_exif` —— PNG 1.5+ `eXIf` chunk payload = 完整
//!   TIFF header（II/MM + `0x002A` magic + IFD0 offset）；调 [`parse_tiff`]。
//! - `entities::exif::image_jpeg::parse_jpeg_app1_exif` —— JPEG APP1 segment
//!   `Exif\0\0` magic 之后是完整 TIFF header；同走 [`parse_tiff`]。
//! - `entities::riff::parse_avi_exif` —— AVI `strd` chunk 是裸 IFD0（无 TIFF
//!   header，固定 LE，offset 基准 = `strd` + 8）；走 [`parse_ifds`]。
//!
//! 仅读归档需要的 5 个 ASCII/LONG 标签（Make/Model/DTO/CreateDate/ModifyDate
//! + `ExifIFDPointer` 指针），不实现完整 TIFF（YAGNI）。

/// IFD 字节序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ByteOrder {
    Le,
    Be,
}

// EXIF 标签号与类型（TIFF 6.0 / EXIF 2.31）。
const TAG_MAKE: u16 = 0x010f;
const TAG_MODEL: u16 = 0x0110;
const TAG_MODIFY_DATE: u16 = 0x0132;
const TAG_EXIF_OFFSET: u16 = 0x8769;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_CREATE_DATE: u16 = 0x9004;
const TYPE_ASCII: u16 = 2;
const TYPE_LONG: u16 = 4;

/// TIFF magic（II/MM 字节序读取后均为 `0x002A`）。
const TIFF_MAGIC: u16 = 0x002A;

/// 单个 ASCII 字段长度上限；Make/Model/日期实测均 <64 字节。
const MAX_ASCII_BYTES: usize = 256;

/// 归档相关字段（日期为 EXIF ASCII 原文 `"YYYY:MM:DD HH:MM:SS"`，
/// 相机本地时间无时区；epoch 转换由调用方做）。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TiffIfd {
    pub date_time_original: Option<String>,
    pub create_date: Option<String>,
    pub modify_date: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
}

/// 完整 TIFF header 入口：`II`/`MM` byte order + `0x002A` magic + IFD0 offset。
/// 损坏头部一律返 None；IFD 内字段全空也返 `Some(TiffIfd::default())`，
/// 由调用方决定是否进一步 fallback。
pub(crate) fn parse_tiff(payload: &[u8]) -> Option<TiffIfd> {
    let bom = payload.get(..2)?;
    let order = match bom {
        b"II" => ByteOrder::Le,
        b"MM" => ByteOrder::Be,
        _ => return None,
    };
    if u16_at(payload, 2, order)? != TIFF_MAGIC {
        return None;
    }
    let ifd0_off = u32_at(payload, 4, order)? as usize;
    parse_ifds(payload, ifd0_off, order)
}

/// 裸 IFD 入口（无 TIFF header）：从 `base[ifd0_off..]` 扫 IFD0，
/// 命中 `ExifIFDPointer` 后续扫 ExifIFD（失败不影响已收集字段）。
pub(crate) fn parse_ifds(base: &[u8], ifd0_off: usize, order: ByteOrder) -> Option<TiffIfd> {
    let mut out = TiffIfd::default();
    let mut exif_ifd: Option<usize> = None;
    scan_ifd(base, ifd0_off, order, &mut out, &mut exif_ifd)?;
    if let Some(off) = exif_ifd {
        let _ = scan_ifd(base, off, order, &mut out, &mut exif_ifd);
    }
    Some(out)
}

// 扫单个 IFD 填充 out；外参 `exif_ifd` 在命中 ExifOffset tag 时被写入。
// 返回 `Option<()>` 表达"IFD count 字段本身可读"——只要 count 能读出即返 Some，
// 中段 entry 越界用 break 截断，已收集字段保留：截断 fixture（PNG eXIf chunk
// 长度声明 N entries 但实际 buffer 只够前 K 条）下，前 K 条的 Make/Model/DTO
// 仍可消费，否则 caller 退到 mtime P4 归错桶。
fn scan_ifd(
    base: &[u8],
    ifd_off: usize,
    order: ByteOrder,
    out: &mut TiffIfd,
    exif_ifd: &mut Option<usize>,
) -> Option<()> {
    let count = u16_at(base, ifd_off, order)? as usize;
    for i in 0..count {
        let e = ifd_off + 2 + i * 12;
        let Some(tag) = u16_at(base, e, order) else {
            break;
        };
        let Some(typ) = u16_at(base, e + 2, order) else {
            break;
        };
        let Some(cnt) = u32_at(base, e + 4, order).map(|v| v as usize) else {
            break;
        };
        let Some(val) = u32_at(base, e + 8, order).map(|v| v as usize) else {
            break;
        };
        match (tag, typ) {
            (TAG_EXIF_OFFSET, TYPE_LONG) => *exif_ifd = Some(val),
            (TAG_MAKE, TYPE_ASCII) => out.make = read_ascii(base, val, cnt),
            (TAG_MODEL, TYPE_ASCII) => out.model = read_ascii(base, val, cnt),
            (TAG_DATE_TIME_ORIGINAL, TYPE_ASCII) => {
                out.date_time_original = read_ascii(base, val, cnt);
            }
            (TAG_CREATE_DATE, TYPE_ASCII) => out.create_date = read_ascii(base, val, cnt),
            (TAG_MODIFY_DATE, TYPE_ASCII) => out.modify_date = read_ascii(base, val, cnt),
            _ => {}
        }
    }
    Some(())
}

// ASCII 字段读取：目标标签（日期 20 字节、Make/Model >4 字节）均走 offset
// 间接存储；cnt ≤ 4 的内联形式对这些标签不会出现，按损坏数据拒绝。
fn read_ascii(base: &[u8], off: usize, cnt: usize) -> Option<String> {
    if cnt <= 4 || cnt > MAX_ASCII_BYTES {
        return None;
    }
    let raw = base.get(off..off + cnt)?;
    let s = std::str::from_utf8(raw)
        .ok()?
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!s.is_empty()).then_some(s)
}

fn u16_at(b: &[u8], off: usize, order: ByteOrder) -> Option<u16> {
    let bytes = b.get(off..off + 2)?;
    Some(match order {
        ByteOrder::Le => u16::from_le_bytes([bytes[0], bytes[1]]),
        ByteOrder::Be => u16::from_be_bytes([bytes[0], bytes[1]]),
    })
}

fn u32_at(b: &[u8], off: usize, order: ByteOrder) -> Option<u32> {
    let bytes = b.get(off..off + 4)?;
    Some(match order {
        ByteOrder::Le => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        ByteOrder::Be => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

#[cfg(test)]
#[path = "tiff_ifd_tests.rs"]
mod tests;
