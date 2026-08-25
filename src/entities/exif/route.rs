//! `Exif::from_reader` 的 MIME 分流路由：按容器类型分发到各 populate 函数。
//! 从 `types.rs` 拆出（原 327 行 → 各文件 ≤300）。

use chrono::FixedOffset;

use super::super::document::populate_document_dates;
use super::super::image::populate_image_dates;
use super::super::image_png::populate_png_dates;
use super::super::image_rw2::populate_rw2_dates;
use super::super::mime::META_TYPE_IMAGE;
use super::super::mime::META_TYPE_VIDEO;
use super::super::mime::MIME_AVI;
use super::super::mime::MIME_M2TS;
use super::super::mime::MIME_PNG;
use super::super::mime::MIME_RW2;
use super::super::mime::is_office_mime;
use super::super::video::populate_avi_dates;
use super::super::video::populate_m2ts_dates;
use super::super::video::populate_video_dates;
use super::Exif;
use crate::entities::backend::MediaReader;

/// 用调用方已 sniff 好的 MIME + 已 seek 到起点的 reader 解析容器内时间。
/// 不再触碰 IO 入口，便于 fake backend 单测各种 MIME 分支。
///
/// `mut reader`：image/video/png/avi/m2ts 分支按 owned Box move 消费；office
/// 分支按 `&mut dyn MediaReader` 借出（stub 阶段不读 reader，commit 接入后子模块
/// 自行读取 ZIP/PDF/CFB 字节）。
pub(super) fn route_reader(
    mut reader: Box<dyn MediaReader>,
    mime_type: &str,
    local_offset: FixedOffset,
) -> Exif {
    let mut exif = Exif {
        mime_type: mime_type.to_string(),
        ..Default::default()
    };
    if mime_type.starts_with(MIME_PNG) {
        // PNG 先于泛 image 分流：nom-exif 不解析 `eXIf` chunk，
        // 时间在 PNG 1.5+ 自定义 chunk 内的完整 TIFF/EXIF header。
        populate_png_dates(reader, &mut exif, local_offset);
    } else if mime_type.starts_with(MIME_RW2) {
        // RW2 先于泛 image 分流：nom-exif 不识 TIFF 变体 magic（0x0055），
        // 时间在 IFD0/ExifIFD（与 TIFF 同布局），走 `entities::rw2` 自解析。
        populate_rw2_dates(reader, &mut exif, local_offset);
    } else if mime_type.starts_with(META_TYPE_IMAGE) {
        populate_image_dates(reader, &mut exif, local_offset);
    } else if mime_type.starts_with(MIME_AVI) {
        // AVI 先于泛 video 分流：nom-exif 不认 RIFF，时间在 strd 内嵌 EXIF。
        populate_avi_dates(reader, &mut exif, local_offset);
    } else if mime_type.starts_with(MIME_M2TS) {
        // M2TS 先于泛 video 分流：nom-exif 不认 MPEG-TS，时间在 H.264 SEI MDPM。
        populate_m2ts_dates(reader, &mut exif, local_offset);
    } else if mime_type.starts_with(META_TYPE_VIDEO) {
        populate_video_dates(reader, &mut exif, local_offset);
    } else if is_office_mime(mime_type) {
        // 办公文档（PDF / OOXML / CFB / iWork / ODF / RTF / EPUB / 思维导图 /
        // 纯文本）走独立分流，不读 EXIF / XMP；子模块返 (created, modified)
        // 二元组归一为 Unix UTC epoch。`reader.as_mut()` 借引用，函数末尾 Drop。
        populate_document_dates(reader.as_mut(), mime_type, &mut exif);
    }
    exif
}
