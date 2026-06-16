//! PNG 容器内嵌 EXIF 解析（PNG 1.5+ `eXIf` chunk）。
//!
//! nom-exif 3.6 不解析 PNG `eXIf` chunk（W3C PNG 1.5 标准、内嵌完整 TIFF/EXIF
//! header，与 JPEG APP1 `Exif\0\0` 之后段同结构）；带 EXIF 的 PNG（如 Lightroom
//! 导出、部分相机直出 PNG）会丢失 DTO/CreateDate 退到 mtime 兜底。
//!
//! 本模块独立解析：遍历 chunk 找 `eXIf` → payload 是完整 TIFF header → 交给
//! `entities::tiff_ifd::parse_tiff`（与 JPEG APP1 fallback 共享）。
//! 不验证 chunk CRC（YAGNI，与 `entities::riff` 同风格）。

use std::io;

use super::backend::MediaReader;
use super::tiff_ifd;

/// PNG 8 字节 signature。
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// PNG chunk type 字节数。
const CHUNK_TYPE_LEN: usize = 4;
/// PNG chunk header 长度 = 4 (data length BE u32) + 4 (chunk type)。
const CHUNK_HEADER_LEN: usize = 8;
/// PNG chunk trailing CRC 长度。
const CHUNK_CRC_LEN: usize = 4;

/// 单个 chunk payload 上限，防恶意 chunk size 字段吃内存。
/// 真实 eXIf 内嵌 EXIF 远 < 64 KiB（IFD 体量级）。
const MAX_CHUNK_BYTES: usize = 1 << 20;

/// 最大扫描 chunk 数（防止恶意/损坏 PNG 无限循环）。
const MAX_CHUNKS: usize = 64;

const CHUNK_TYPE_EXIF: &[u8; CHUNK_TYPE_LEN] = b"eXIf";
const CHUNK_TYPE_IEND: &[u8; CHUNK_TYPE_LEN] = b"IEND";

/// PNG 归档相关字段；与 `tiff_ifd::TiffIfd` 同 schema，由后者直接复用。
pub(crate) type PngExif = tiff_ifd::TiffIfd;

/// 从 PNG reader（须位于流起点）提取 `eXIf` chunk 内 TIFF/EXIF 字段。
/// 非 PNG / 无 `eXIf` chunk / 结构损坏一律返回 None。
pub(crate) fn parse_png_exif(r: &mut dyn MediaReader) -> Option<PngExif> {
    let mut sig = [0u8; 8];
    r.read_exact(&mut sig).ok()?;
    if &sig != PNG_SIGNATURE {
        return None;
    }
    for _ in 0..MAX_CHUNKS {
        let mut hdr = [0u8; CHUNK_HEADER_LEN];
        r.read_exact(&mut hdr).ok()?;
        let len = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        let chunk_type = &hdr[4..8];
        if chunk_type == CHUNK_TYPE_EXIF {
            if len > MAX_CHUNK_BYTES {
                return None;
            }
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload).ok()?;
            return tiff_ifd::parse_tiff(&payload);
        }
        if chunk_type == CHUNK_TYPE_IEND {
            return None;
        }
        // 非目标 chunk：跳过 data + CRC（i64 转换不会溢出，u32 上限 4 GiB）。
        let skip = i64::try_from(len + CHUNK_CRC_LEN).unwrap_or(i64::MAX);
        r.seek(io::SeekFrom::Current(skip)).ok()?;
    }
    None
}

#[cfg(test)]
#[path = "png_tests.rs"]
mod tests;
