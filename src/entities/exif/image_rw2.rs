//! Panasonic RW2 RAW 内嵌 EXIF 解析 + XMP fallback 双轨写入 [`Exif`]。
//!
//! 走自实现路径（不经 nom-exif）原因见 `entities::rw2` 模块注释。
//! 解析成功填 P0/P1（DTO/CreateDate）+ ModifyDate（仲裁旁证）+ Make/Model；
//! 未命中 / 结构损坏退回 `populate_image_xmp_fallback`。只碰前缀窗口，
//! 不整读 RAW（≥20 MB，远端 `open_read` 已整文件下载）。

use std::io;

use chrono::FixedOffset;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;
use super::super::rw2;
use super::image::apply_tiff_ifd;
use super::image::populate_image_xmp_fallback;
use super::image::populate_image_xmp_fallback_if_empty;
use super::types::Exif;

/// XMP packet fallback 扫描窗口（与 `image.rs::XMP_SCAN_BYTES` 同口径）。
const XMP_SCAN_BYTES: usize = 64 * 1024;

pub(super) fn populate_rw2_dates(
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

    let Some(ifd) = rw2::parse_rw2_exif(&head) else {
        populate_image_xmp_fallback(&head, exif);
        return;
    };
    apply_tiff_ifd(exif, ifd, local_offset);

    populate_image_xmp_fallback_if_empty(&head, exif);
}
