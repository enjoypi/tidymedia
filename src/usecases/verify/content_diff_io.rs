use image::{DynamicImage, GenericImageView, GrayImage, Rgb, RgbImage};

use crate::entities::backend::{Backend, FileBytes};
use crate::entities::uri::Location;

pub(super) fn read_to_end(backend: &dyn Backend, loc: &Location, size: u64) -> Option<FileBytes> {
    if let Ok(Some(mapped)) = backend.map_whole(loc) {
        return Some(FileBytes::Mapped(mapped));
    }
    let mut reader = backend.open_read(loc).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    std::io::Read::read_to_end(&mut reader, &mut buf).ok()?;
    Some(FileBytes::Heap(buf))
}

pub(super) fn grayscale(image: RgbImage) -> GrayImage {
    image::imageops::grayscale(&DynamicImage::ImageRgb8(image))
}

pub(super) fn resize<V: GenericImageView<Pixel = Rgb<u8>>>(image: &V, side: u32) -> RgbImage {
    image::imageops::resize(image, side, side, image::imageops::FilterType::Triangle)
}
