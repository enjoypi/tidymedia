use std::sync::OnceLock;

use image::RgbImage;

use crate::entities::SecureHash;
use crate::entities::backend::FileBytes;
use crate::usecases::image_input::{DecodeCtx, DecodeError, with_rgb};

use super::content_diff_io::resize;
use super::phash::{hamming, phash};
use super::rotated_view::RotatedView;

const ROTATIONS: usize = 4;
const NORMALIZE_SIDE: u32 = 512;
const MEAN_ABS_DIFF_MAX: f32 = 5.0;

enum Decoded<T> {
    Ready(T),
    Invalid,
    TooLarge,
}

impl<T> Decoded<T> {
    fn from_result(result: Result<T, DecodeError>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(DecodeError::TooLarge(_)) => Self::TooLarge,
            Err(DecodeError::Invalid(_)) => Self::Invalid,
        }
    }

    fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            Self::Invalid | Self::TooLarge => None,
        }
    }

    fn too_large(&self) -> bool {
        matches!(self, Self::TooLarge)
    }
}

struct ImageFeatures {
    width: u32,
    height: u32,
}

pub(crate) struct PreparedSource<'a> {
    bytes: &'a [u8],
    ctx: DecodeCtx<'a>,
    image: OnceLock<Decoded<(ImageFeatures, [u64; ROTATIONS])>>,
    normalized: [OnceLock<Option<RgbImage>>; ROTATIONS],
}

impl<'a> PreparedSource<'a> {
    pub(crate) fn new(bytes: &'a [u8], ctx: DecodeCtx<'a>) -> Self {
        Self {
            bytes,
            ctx,
            image: OnceLock::new(),
            normalized: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    fn image(&self) -> &Decoded<(ImageFeatures, [u64; ROTATIONS])> {
        self.image.get_or_init(|| {
            Decoded::from_result(with_rgb(self.bytes, self.ctx, NORMALIZE_SIDE, |rgb| {
                let (width, height) = rgb.dimensions();
                let hashes = std::array::from_fn(|turns| phash(&RotatedView::new(rgb, turns)));
                (ImageFeatures { width, height }, hashes)
            }))
        })
    }

    fn normalized(&self, turns: usize) -> Option<&RgbImage> {
        self.normalized[turns]
            .get_or_init(|| {
                with_rgb(self.bytes, self.ctx, NORMALIZE_SIDE, |rgb| {
                    resize(&RotatedView::new(rgb, turns), NORMALIZE_SIDE)
                })
                .ok()
            })
            .as_ref()
    }

    pub(crate) fn too_large(&self) -> bool {
        self.image.get().is_some_and(Decoded::too_large)
    }
}

pub(super) struct CandidateFeatures {
    pub(super) entropy: Option<SecureHash>,
    image: Decoded<(ImageFeatures, u64)>,
}

impl CandidateFeatures {
    pub(super) fn from_bytes(bytes: &[u8], ext: &str, ctx: DecodeCtx<'_>) -> Self {
        let entropy = super::content_diff::entropy_hash(bytes, ext);
        let image = Decoded::from_result(with_rgb(bytes, ctx, NORMALIZE_SIDE, |rgb| {
            let (width, height) = rgb.dimensions();
            (ImageFeatures { width, height }, phash(rgb))
        }));
        Self { entropy, image }
    }

    pub(super) fn too_large(&self) -> bool {
        self.image.too_large()
    }
}

pub(super) fn rotated_candidate_similar(
    source: &PreparedSource<'_>,
    candidate: &CandidateFeatures,
    max_hamming: u8,
    read_candidate: &dyn Fn() -> Option<FileBytes>,
) -> bool {
    let Some((candidate_image, candidate_hash)) = candidate.image.ready() else {
        return false;
    };
    let Some((source_image, source_hashes)) = source.image().ready() else {
        return false;
    };
    let (wa, ha) = (source_image.width, source_image.height);
    if (wa, ha) != (candidate_image.width, candidate_image.height)
        && (wa, ha) != (candidate_image.height, candidate_image.width)
    {
        return false;
    }
    let mut best = 0_usize;
    for i in 1..source_hashes.len() {
        if hamming(source_hashes[i], *candidate_hash)
            < hamming(source_hashes[best], *candidate_hash)
        {
            best = i;
        }
    }
    if hamming(source_hashes[best], *candidate_hash) > u32::from(max_hamming) {
        return false;
    }
    let Some(bytes) = read_candidate() else {
        return false;
    };
    let Ok(candidate_normalized) = with_rgb(&bytes, source.ctx, NORMALIZE_SIDE, |rgb| {
        resize(rgb, NORMALIZE_SIDE)
    }) else {
        return false;
    };
    if !bytes.unchanged() {
        return false;
    }
    source.normalized(best).is_some_and(|source_normalized| {
        mean_abs_diff(source_normalized, &candidate_normalized) < MEAN_ABS_DIFF_MAX
    })
}

#[cfg(test)]
pub(crate) fn rotated_phash_similar(a: &[u8], b: &[u8], max_hamming: u8) -> bool {
    let ctx = DecodeCtx::global();
    let source = PreparedSource::new(a, ctx);
    let candidate = CandidateFeatures::from_bytes(b, "", ctx);
    rotated_candidate_similar(&source, &candidate, max_hamming, &|| {
        Some(FileBytes::Heap(b.to_vec()))
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "sum ≤ 255×3×512×512 ≈ 2×10^8，超 f32 尾数但 mean 阈值 5 的量级判定不受影响"
)]
fn mean_abs_diff(a: &RgbImage, b: &RgbImage) -> f32 {
    let mut sum = 0_u64;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        for ch in 0..3 {
            sum += u64::from((i32::from(pa.0[ch]) - i32::from(pb.0[ch])).unsigned_abs());
        }
    }
    sum as f32 / (NORMALIZE_SIDE * NORMALIZE_SIDE * 3) as f32
}

#[cfg(test)]
#[path = "content_features_tests.rs"]
mod tests;
