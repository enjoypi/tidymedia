use std::sync::OnceLock;

use image::RgbImage;

use crate::entities::SecureHash;

use super::phash::{hamming, phash};

pub(crate) struct PreparedSource<'a> {
    bytes: &'a [u8],
    image: OnceLock<Option<SourceImage>>,
}

pub(crate) struct SourceImage {
    rotations: [RgbImage; 4],
    hashes: [u64; 4],
}

impl<'a> PreparedSource<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            image: OnceLock::new(),
        }
    }

    pub(crate) fn image(&self) -> Option<&SourceImage> {
        self.image
            .get_or_init(|| {
                let img = super::content_diff_io::load(self.bytes).ok()?;
                let rgb = img.to_rgb8();
                let a90 = super::content_diff_io::rotate90(&rgb);
                let a180 = super::content_diff_io::rotate180(&rgb);
                let a270 = super::content_diff_io::rotate270(&rgb);
                let rotations = [rgb, a90, a180, a270];
                let hashes = std::array::from_fn(|i| phash(&rotations[i]));
                Some(SourceImage { rotations, hashes })
            })
            .as_ref()
    }
}

pub(super) struct CandidateFeatures {
    pub(super) entropy: Option<SecureHash>,
    image: Option<ImageFeatures>,
}

struct ImageFeatures {
    width: u32,
    height: u32,
    hash: u64,
}

impl CandidateFeatures {
    pub(super) fn from_bytes(bytes: &[u8], ext: &str) -> Self {
        let entropy = super::content_diff::entropy_hash(bytes, ext);
        let image = super::content_diff_io::load(bytes).ok().map(|img| {
            let width = img.width();
            let height = img.height();
            let hash = phash(&img.to_rgb8());
            ImageFeatures {
                width,
                height,
                hash,
            }
        });
        Self { entropy, image }
    }
}

pub(super) fn rotated_candidate_similar(
    source: &PreparedSource<'_>,
    candidate: &CandidateFeatures,
    max_hamming: u8,
    read_candidate: &dyn Fn() -> Option<Vec<u8>>,
) -> bool {
    let Some(candidate_image) = candidate.image.as_ref() else {
        return false;
    };
    let Some(source_image) = source.image() else {
        return false;
    };
    let (wa, ha) = (
        source_image.rotations[0].width(),
        source_image.rotations[0].height(),
    );
    if (wa, ha) != (candidate_image.width, candidate_image.height)
        && (wa, ha) != (candidate_image.height, candidate_image.width)
    {
        return false;
    }
    let mut best = 0_usize;
    for i in 1..source_image.hashes.len() {
        if hamming(source_image.hashes[i], candidate_image.hash)
            < hamming(source_image.hashes[best], candidate_image.hash)
        {
            best = i;
        }
    }
    if hamming(source_image.hashes[best], candidate_image.hash) > u32::from(max_hamming) {
        return false;
    }
    let Some(bytes) = read_candidate() else {
        return false;
    };
    let Ok(img) = super::content_diff_io::load(&bytes) else {
        return false;
    };
    mean_abs_diff_normalized(&source_image.rotations[best], &img.to_rgb8()) < MEAN_ABS_DIFF_MAX
}

#[cfg(test)]
pub(crate) fn rotated_phash_similar(a: &[u8], b: &[u8], max_hamming: u8) -> bool {
    let source = PreparedSource::new(a);
    let candidate = CandidateFeatures::from_bytes(b, "");
    rotated_candidate_similar(&source, &candidate, max_hamming, &|| Some(b.to_vec()))
}

const NORMALIZE_SIDE: u32 = 512;
const MEAN_ABS_DIFF_MAX: f32 = 5.0;

#[expect(
    clippy::cast_precision_loss,
    reason = "sum ≤ 255×3×512×512 ≈ 2×10^8，超 f32 尾数但 mean 阈值 5 的量级判定不受影响"
)]
fn mean_abs_diff_normalized(a: &RgbImage, b: &RgbImage) -> f32 {
    let ra = super::content_diff_io::resize(a, NORMALIZE_SIDE);
    let rb = super::content_diff_io::resize(b, NORMALIZE_SIDE);
    let mut sum = 0_u64;
    for (pa, pb) in ra.pixels().zip(rb.pixels()) {
        for ch in 0..3 {
            sum += u64::from((i32::from(pa.0[ch]) - i32::from(pb.0[ch])).unsigned_abs());
        }
    }
    sum as f32 / (NORMALIZE_SIDE * NORMALIZE_SIDE * 3) as f32
}

#[cfg(test)]
#[path = "content_features_tests.rs"]
mod tests;
