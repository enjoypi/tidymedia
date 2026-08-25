//! Panasonic RW2 RAW 容器内嵌 EXIF 解析（TIFF 变体，magic `0x0055`）。
//!
//! RW2 header 与 TIFF 同布局（BOM + magic + IFD0 offset）但 magic 第三字节
//! 为 `0x55`（`II U\0`）非 TIFF `0x2A`，`tiff_ifd::parse_tiff` 硬校验必 None。
//! 走参数化入口 `tiff_ifd::parse_tiff_with_magic` 复用 IFD0 + `ExifIFD` 扫描。

use super::tiff_ifd;

/// 从 RW2 前缀字节提取归档字段。`head` 须含 IFD0 与 ExifIFD（真实 RW2 的
/// IFD0 紧跟 header 后；fixture 造在 offset=8）。结构损坏返 None。
pub(crate) fn parse_rw2_exif(head: &[u8]) -> Option<tiff_ifd::TiffIfd> {
    tiff_ifd::parse_tiff_with_magic(head, tiff_ifd::RW2_MAGIC)
}

#[cfg(test)]
#[path = "rw2_tests.rs"]
mod tests;
