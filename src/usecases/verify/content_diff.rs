use sha2::{Digest, Sha512};

use crate::entities::SecureHash;
use crate::entities::file_info::Info;

pub(crate) use super::content_features::PreparedSource;
use super::content_features::rotated_candidate_similar;
#[cfg(test)]
pub(crate) use super::content_features::rotated_phash_similar;
pub(crate) use super::output_library::OutputLibrary;
#[cfg(test)]
pub(crate) use super::output_library::{build_output_index, read_guarded_by_path};

pub(crate) fn verdict_for(
    info: &Info,
    lib: &OutputLibrary,
    phash_max: u8,
    max_bytes: u64,
) -> String {
    if lib.has_exact(info) {
        return "exact_dup".to_owned();
    }
    let base = basename(info.full_path.as_str()).to_owned();
    let cands = lib.candidates(&base);
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
    let source = PreparedSource::new(&src_bytes);
    let mut rotated = false;
    let mut retry = Vec::new();
    for path in cands {
        let Some(features) = lib.features(&path, max_bytes) else {
            retry.push(path);
            continue;
        };
        if src_entropy.is_some() && features.entropy == src_entropy {
            return "pixel_same".to_owned();
        }
        if rotated_candidate_similar(&source, &features, phash_max, &|| {
            lib.read_candidate(&path, max_bytes)
        }) {
            if src_entropy.is_none() {
                return "rotated_same".to_owned();
            }
            rotated = true;
        }
    }
    rotated = retry_if_needed(&source, &retry, lib, phash_max, max_bytes, rotated);
    if rotated {
        "rotated_same".to_owned()
    } else {
        "name_only".to_owned()
    }
}

fn retry_if_needed(
    source: &PreparedSource<'_>,
    paths: &[camino::Utf8PathBuf],
    lib: &OutputLibrary,
    phash_max: u8,
    max_bytes: u64,
    rotated: bool,
) -> bool {
    rotated || retry_rotated(source, paths, lib, phash_max, max_bytes)
}

fn retry_rotated(
    source: &PreparedSource<'_>,
    paths: &[camino::Utf8PathBuf],
    lib: &OutputLibrary,
    phash_max: u8,
    max_bytes: u64,
) -> bool {
    paths.iter().any(|path| {
        lib.features(path, max_bytes).is_some_and(|features| {
            rotated_candidate_similar(source, &features, phash_max, &|| {
                lib.read_candidate(path, max_bytes)
            })
        })
    })
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

pub(super) fn read_guarded(info: &Info, max_bytes: u64) -> Option<Vec<u8>> {
    if info.size > max_bytes {
        return None;
    }
    super::content_diff_io::read_to_end(info.backend().as_ref(), info.location(), info.size)
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
