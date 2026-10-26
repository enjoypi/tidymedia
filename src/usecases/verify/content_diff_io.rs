use image::{DynamicImage, GrayImage, ImageResult, RgbImage};

use crate::entities::backend::Backend;
use crate::entities::uri::Location;

pub(super) fn read_to_end(backend: &dyn Backend, loc: &Location, size: u64) -> Option<Vec<u8>> {
    let mut reader = backend.open_read(loc).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    std::io::Read::read_to_end(&mut reader, &mut buf).ok()?;
    Some(buf)
}

pub(super) fn load(bytes: &[u8]) -> ImageResult<DynamicImage> {
    image::load_from_memory(bytes)
}

pub(super) fn rotate90(image: &RgbImage) -> RgbImage {
    image::imageops::rotate90(image)
}

pub(super) fn rotate180(image: &RgbImage) -> RgbImage {
    image::imageops::rotate180(image)
}

pub(super) fn rotate270(image: &RgbImage) -> RgbImage {
    image::imageops::rotate270(image)
}

pub(super) fn grayscale(image: RgbImage) -> GrayImage {
    image::imageops::grayscale(&DynamicImage::ImageRgb8(image))
}

pub(super) fn resize(image: &RgbImage, side: u32) -> RgbImage {
    image::imageops::resize(image, side, side, image::imageops::FilterType::Triangle)
}
