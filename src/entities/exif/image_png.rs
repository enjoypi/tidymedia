use chrono::FixedOffset;

use super::super::backend::MediaReader;
use super::super::png;
use super::exif_io;
use super::image::apply_tiff_ifd;
use super::image::populate_image_xmp_fallback;
use super::image::populate_image_xmp_fallback_if_empty;
use super::types::Exif;

const XMP_SCAN_BYTES: usize = 64 * 1024;

pub(super) fn populate_png_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let head = exif_io::read_head(reader.as_mut(), XMP_SCAN_BYTES);
    if exif_io::rewind(reader.as_mut()).is_err() {
        populate_image_xmp_fallback(&head, exif);
        return;
    }

    let Some(ifd) = png::parse_png_exif(reader.as_mut()) else {
        populate_image_xmp_fallback(&head, exif);
        return;
    };
    apply_tiff_ifd(exif, ifd, local_offset);

    populate_image_xmp_fallback_if_empty(&head, exif);
}
