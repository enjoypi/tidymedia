//! PNG `eXIf` chunk 解析 + XMP fallback 双轨写入 [`Exif`]。
//!
//! 走自实现路径（不经 nom-exif）的原因见 `entities::png` 模块注释。
//! 解析成功填 P0/P1（DTO/CreateDate）+ ModifyDate（仲裁旁证）+ Make/Model；
//! 未命中 / `eXIf` 损坏退回 `populate_image_xmp_fallback`（与既有 image 主路径
//! 双 0 兜底语义一致——Lightroom 类导出常同时写 PNG eXIf 与 XMP packet）。

use std::io;

use chrono::FixedOffset;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;
use super::super::png;
use super::image::apply_tiff_ifd;
use super::image::populate_image_xmp_fallback;
use super::image::populate_image_xmp_fallback_if_empty;
use super::types::Exif;

/// XMP packet fallback 扫描窗口（与 `image.rs::XMP_SCAN_BYTES` 同口径）。
const XMP_SCAN_BYTES: usize = 64 * 1024;

pub(super) fn populate_png_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let mut head = vec![0u8; XMP_SCAN_BYTES];
    let head_len = read_fill(reader.as_mut(), &mut head).unwrap_or(0);
    head.truncate(head_len);
    if reader.seek(io::SeekFrom::Start(0)).is_err() {
        populate_image_xmp_fallback(&head, exif);
        return;
    }

    let Some(ifd) = png::parse_png_exif(reader.as_mut()) else {
        populate_image_xmp_fallback(&head, exif);
        return;
    };
    apply_tiff_ifd(exif, ifd, local_offset);

    // PNG eXIf 但所有日期字段全空：仍尝试 XMP fallback（导出工具常并行写）。
    // 复用 image.rs 的 helper 收敛 `&&` 短路 BR 到单点。
    populate_image_xmp_fallback_if_empty(&head, exif);
}
