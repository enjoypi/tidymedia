use camino::Utf8PathBuf;
use sha2::{Digest, Sha512};

use super::phash::{hamming, phash};
use crate::entities::SecureHash;
use crate::entities::backend::EntryKind;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::usecases::copy::Source;

use super::candidates::CandidateIndex;

pub(crate) fn verdict_for(
    info: &Info,
    lib: &OutputLibrary,
    phash_max: u8,
    max_bytes: u64,
) -> String {
    let out = &lib.index;
    if matches!(out.exists(info, true), Ok(Some(_))) {
        return "exact_dup".to_owned();
    }
    let base = basename(info.full_path.as_str()).to_owned();
    let cands = lib.cands.find(&base);
    if cands.is_empty() {
        return "absent".to_owned();
    }
    let Some(src_bytes) = read_guarded(info, max_bytes) else {
        return "name_only".to_owned();
    };
    let ext = info
        .full_path
        .extension()
        .map_or_default(str::to_ascii_lowercase);
    let src_entropy = entropy_hash(&src_bytes, &ext);
    for path in &cands {
        let Some(c_bytes) = read_guarded_by_path(out, path, max_bytes) else {
            continue;
        };
        if src_entropy.is_some() && entropy_hash(&c_bytes, &ext) == src_entropy {
            return "pixel_same".to_owned();
        }
    }
    for path in &cands {
        let Some(c_bytes) = read_guarded_by_path(out, path, max_bytes) else {
            continue;
        };
        if rotated_phash_similar(&src_bytes, &c_bytes, phash_max) {
            return "rotated_same".to_owned();
        }
    }
    "name_only".to_owned()
}

pub(crate) struct OutputLibrary {
    index: Index,
    cands: CandidateIndex,
}

impl OutputLibrary {
    pub(crate) fn new(index: Index) -> Self {
        let cands = CandidateIndex::build(&index);
        Self { index, cands }
    }
}

pub(crate) fn build_output_index(output: &Source) -> Index {
    let mut idx = Index::new();
    let (loc, backend) = output;
    if backend
        .metadata(loc)
        .is_ok_and(|m| m.kind == EntryKind::Dir)
    {
        idx.visit_location(loc, backend);
    }
    idx
}

pub(crate) fn split_stem_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) => (name[..i].to_owned(), name[i..].to_owned()),
        None => (name.to_owned(), String::new()),
    }
}

pub(crate) fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn read_guarded(info: &Info, max_bytes: u64) -> Option<Vec<u8>> {
    if info.size > max_bytes {
        return None;
    }
    super::content_diff_io::read_to_end(info.backend().as_ref(), info.location(), info.size)
}

fn read_guarded_by_path(out: &Index, path: &Utf8PathBuf, max_bytes: u64) -> Option<Vec<u8>> {
    let info = out.get(path)?;
    read_guarded(&info, max_bytes)
}

#[must_use]
pub(crate) fn entropy_hash(bytes: &[u8], ext: &str) -> Option<SecureHash> {
    match ext {
        "jpg" | "jpeg" => jpeg_entropy_hash(bytes).or_else(|| png_idat_hash(bytes)),
        "png" => png_idat_hash(bytes).or_else(|| jpeg_entropy_hash(bytes)),
        "mp4" | "mov" | "m4v" | "3gp" | "heic" | "heif" => bmff_mdat_hash(bytes),
        _ => None,
    }
}

#[must_use]
pub(crate) fn rotated_phash_similar(a: &[u8], b: &[u8], max_hamming: u8) -> bool {
    let (Ok(img_a), Ok(img_b)) = (
        super::content_diff_io::load(a),
        super::content_diff_io::load(b),
    ) else {
        return false;
    };
    let (wa, ha) = (img_a.width(), img_a.height());
    let (wb, hb) = (img_b.width(), img_b.height());
    if (wa, ha) != (wb, hb) && (wa, ha) != (hb, wb) {
        return false;
    }
    let a_rgb = img_a.to_rgb8();
    let b_rgb = img_b.to_rgb8();
    let h_b = phash(&b_rgb);
    let a90 = super::content_diff_io::rotate90(&a_rgb);
    let a180 = super::content_diff_io::rotate180(&a_rgb);
    let a270 = super::content_diff_io::rotate270(&a_rgb);
    let rotations = [&a_rgb, &a90, &a180, &a270];
    let (best_img, best_h) = rotations
        .iter()
        .map(|img| (img, hamming(phash(img), h_b)))
        .min_by_key(|(_, h)| *h)
        .expect("internal: rotations non-empty");
    if best_h > u32::from(max_hamming) {
        return false;
    }
    mean_abs_diff_normalized(best_img, &b_rgb) < MEAN_ABS_DIFF_MAX
}

const NORMALIZE_SIDE: u32 = 512;
const MEAN_ABS_DIFF_MAX: f32 = 5.0;

#[expect(
    clippy::cast_precision_loss,
    reason = "sum ≤ 255×3×512×512 ≈ 2×10^8，超 f32 尾数但 mean 阈值 5 的量级判定不受影响"
)]
fn mean_abs_diff_normalized(a: &image::RgbImage, b: &image::RgbImage) -> f32 {
    let ra = super::content_diff_io::resize(a, NORMALIZE_SIDE);
    let rb = super::content_diff_io::resize(b, NORMALIZE_SIDE);
    let mut sum = 0u64;
    for (pa, pb) in ra.pixels().zip(rb.pixels()) {
        for ch in 0..3 {
            sum += u64::from((i32::from(pa.0[ch]) - i32::from(pb.0[ch])).unsigned_abs());
        }
    }
    sum as f32 / (NORMALIZE_SIDE * NORMALIZE_SIDE * 3) as f32
}

#[must_use]
pub(crate) fn jpeg_entropy_hash(bytes: &[u8]) -> Option<SecureHash> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && bytes[j] == 0xFF {
            j += 1;
        }
        if j >= bytes.len() {
            return None;
        }
        let marker = bytes[j];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i = j + 1;
            continue;
        }
        if j + 2 > bytes.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([bytes[j + 1], bytes[j + 2]]) as usize;
        if marker == 0xDA {
            return Some(hash_rest(bytes, j + 1 + seg_len));
        }
        i = j + 1 + seg_len;
    }
    None
}

#[must_use]
pub(crate) fn png_idat_hash(bytes: &[u8]) -> Option<SecureHash> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIG) {
        return None;
    }
    let mut h = Sha512::new();
    let mut found = false;
    let mut pos = SIG.len();
    while pos + 8 <= bytes.len() {
        let ln = be_u32(bytes, pos) as usize;
        let typ = &bytes[pos + 4..pos + 8];
        if typ == b"IDAT" {
            found = true;
            h.update(bytes.get(pos + 8..pos + 8 + ln)?);
        }
        pos += 12 + ln;
        if typ == b"IEND" {
            break;
        }
    }
    found.then(|| h.finalize())
}

fn be_u32(bytes: &[u8], pos: usize) -> u32 {
    u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

#[must_use]
pub(crate) fn bmff_mdat_hash(bytes: &[u8]) -> Option<SecureHash> {
    let mut h = Sha512::new();
    let mut found = false;
    let mut pos = 0;
    while pos + 8 <= bytes.len() {
        let ln32 = be_u32(bytes, pos);
        let typ = &bytes[pos + 4..pos + 8];
        let (ln, hdr_len) = match ln32 {
            1 => {
                let wide = u64::from_be_bytes(*bytes[pos + 8..].first_chunk::<8>()?);
                (wide, 16)
            }
            0 => (u64::try_from(bytes.len() - pos).unwrap_or(u64::MAX), 8),
            n => (u64::from(n), 8),
        };
        if ln < 8 {
            break;
        }
        if typ == b"mdat" {
            found = true;
            let start = pos + hdr_len;
            let end = pos
                .saturating_add(usize::try_from(ln).unwrap_or(usize::MAX))
                .min(bytes.len());
            h.update(bytes.get(start..end)?);
        }
        pos = pos.saturating_add(usize::try_from(ln).unwrap_or(usize::MAX));
    }
    found.then(|| h.finalize())
}

#[must_use]
pub(crate) fn hash_rest(bytes: &[u8], from: usize) -> SecureHash {
    let mut h = Sha512::new();
    h.update(bytes.get(from..).unwrap_or(&[]));
    h.finalize()
}

#[cfg(test)]
use self::tests_common::sha512_bytes;
#[cfg(test)]
use super::content_diff_io::read_to_end;

#[cfg(test)]
#[path = "content_diff_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "content_diff_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "content_diff_tests.rs"]
mod tests;
