use sha2::{Digest, Sha512};

use crate::entities::SecureHash;

#[must_use]
pub(crate) fn sha512_bytes(bytes: &[u8]) -> SecureHash {
    let mut h = Sha512::new();
    h.update(bytes);
    h.finalize()
}

pub(crate) fn legacy_rotated_similar(a: &[u8], b: &[u8], max_hamming: u8) -> bool {
    use image::imageops::{FilterType, resize, rotate90, rotate180, rotate270};

    use super::super::phash::{hamming, phash};

    let (Ok(source), Ok(candidate)) = (image::load_from_memory(a), image::load_from_memory(b))
    else {
        return false;
    };
    let rgb = source.to_rgb8();
    let rotations = [
        rgb.clone(),
        rotate90(&rgb),
        rotate180(&rgb),
        rotate270(&rgb),
    ];
    let candidate = candidate.to_rgb8();
    let (wa, ha) = rgb.dimensions();
    let (wb, hb) = candidate.dimensions();
    if (wa, ha) != (wb, hb) && (wa, ha) != (hb, wb) {
        return false;
    }
    let hash = phash(&candidate);
    let mut best = 0_usize;
    for i in 1..rotations.len() {
        if hamming(phash(&rotations[i]), hash) < hamming(phash(&rotations[best]), hash) {
            best = i;
        }
    }
    if hamming(phash(&rotations[best]), hash) > u32::from(max_hamming) {
        return false;
    }
    let ra = resize(&rotations[best], 512, 512, FilterType::Triangle);
    let rb = resize(&candidate, 512, 512, FilterType::Triangle);
    let mut sum = 0_u64;
    for (pa, pb) in ra.pixels().zip(rb.pixels()) {
        for ch in 0..3 {
            sum += u64::from((i32::from(pa.0[ch]) - i32::from(pb.0[ch])).unsigned_abs());
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "与生产实现同一口径")]
    let mean = sum as f32 / (512_u32 * 512 * 3) as f32;
    mean < 5.0
}
