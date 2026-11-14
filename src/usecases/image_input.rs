use image::RgbImage;

use crate::entities::memory::{Budget, Reservation, TooLarge};

#[path = "image_input_io.rs"]
mod io;

const RESIZE_BYTES_PER_SAMPLE_ROW: u64 = 16;
const RGB_BYTES_PER_PIXEL: u64 = 3;
const DECODER_BASE_BYTES: u64 = 1 << 20;

fn decode_alloc(header: &Header, factor: u64) -> u64 {
    header
        .total_bytes
        .saturating_mul(factor)
        .saturating_add(DECODER_BASE_BYTES)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub width: u32,
    pub height: u32,
    pub total_bytes: u64,
    pub rgb8: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DecodeError {
    TooLarge(TooLarge),
    Invalid(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(e) => write!(f, "image too large: {e}"),
            Self::Invalid(e) => write!(f, "decode image: {e}"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DecodeCtx<'a> {
    pub work: &'a Budget,
    pub factor: u64,
}

impl DecodeCtx<'static> {
    pub(crate) fn global() -> Self {
        Self {
            work: &crate::usecases::memory::governor().work,
            factor: crate::usecases::config::config().memory.decode_factor,
        }
    }
}

#[must_use]
pub(crate) fn estimate(header: &Header, factor: u64, resize_side: u32) -> u64 {
    let pixels = u64::from(header.width) * u64::from(header.height);
    let convert = if header.rgb8 {
        0
    } else {
        pixels.saturating_mul(RGB_BYTES_PER_PIXEL)
    };
    let resize = u64::from(header.width.max(header.height))
        * u64::from(resize_side)
        * RESIZE_BYTES_PER_SAMPLE_ROW;
    decode_alloc(header, factor)
        .saturating_add(convert)
        .saturating_add(resize)
}

struct Held<'a> {
    rgb: RgbImage,
    _reservation: Reservation<'a>,
}

fn decode<'a>(bytes: &[u8], ctx: DecodeCtx<'a>, resize_side: u32) -> Result<Held<'a>, DecodeError> {
    let header = io::header(bytes)?;
    let need = estimate(&header, ctx.factor, resize_side);
    let reservation = ctx.work.reserve(need).map_err(DecodeError::TooLarge)?;
    let rgb = io::decode_rgb(bytes, decode_alloc(&header, ctx.factor))?;
    Ok(Held {
        rgb,
        _reservation: reservation,
    })
}

pub(crate) fn with_rgb<R>(
    bytes: &[u8],
    ctx: DecodeCtx<'_>,
    resize_side: u32,
    f: impl FnOnce(&RgbImage) -> R,
) -> Result<R, DecodeError> {
    decode(bytes, ctx, resize_side).map(|held| f(&held.rgb))
}

pub(crate) const VERIFY_RESIZE_SIDE: u32 = 512;

pub(crate) fn calibrate(bytes: &[u8]) -> Option<u64> {
    let header = io::header(bytes).ok()?;
    let factor = crate::usecases::config::config().memory.decode_factor;
    let work = Budget::new(crate::entities::memory::Level::Work, u64::MAX);
    let ctx = DecodeCtx {
        work: &work,
        factor,
    };
    with_rgb(bytes, ctx, VERIFY_RESIZE_SIDE, |rgb| {
        image::imageops::resize(
            rgb,
            VERIFY_RESIZE_SIDE,
            VERIFY_RESIZE_SIDE,
            image::imageops::FilterType::Triangle,
        )
        .len()
    })
    .ok()?;
    Some(estimate(&header, factor, VERIFY_RESIZE_SIDE))
}

#[cfg(test)]
#[path = "image_input_tests.rs"]
mod tests;
