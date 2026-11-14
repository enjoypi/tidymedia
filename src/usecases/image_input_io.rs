use std::io::Cursor;

use image::error::ImageError;
use image::{ColorType, ImageDecoder, ImageReader, Limits, RgbImage};

use super::{DecodeError, Header};
use crate::entities::memory::TooLarge;

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, DecodeError> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DecodeError::Invalid(e.to_string()))
}

fn classify(error: &ImageError, max_alloc: u64) -> DecodeError {
    match error {
        ImageError::Limits(_) => DecodeError::TooLarge(TooLarge {
            need: max_alloc.saturating_add(1),
            capacity: max_alloc,
        }),
        other => DecodeError::Invalid(other.to_string()),
    }
}

pub(super) fn header(bytes: &[u8]) -> Result<Header, DecodeError> {
    let mut reader = reader(bytes)?;
    reader.no_limits();
    let decoder = reader.into_decoder().map_err(|e| classify(&e, u64::MAX))?;
    let (width, height) = decoder.dimensions();
    Ok(Header {
        width,
        height,
        total_bytes: decoder.total_bytes(),
        rgb8: decoder.color_type() == ColorType::Rgb8,
    })
}

pub(super) fn decode_rgb(bytes: &[u8], max_alloc: u64) -> Result<RgbImage, DecodeError> {
    let mut reader = reader(bytes)?;
    let mut limits = Limits::no_limits();
    limits.max_alloc = Some(max_alloc);
    reader.limits(limits);
    reader
        .decode()
        .map(image::DynamicImage::into_rgb8)
        .map_err(|e| classify(&e, max_alloc))
}
