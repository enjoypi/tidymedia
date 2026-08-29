//! copied 内容比对纯函数 + verdict 判定（口径与 tidy-verify skill 一致）：`SHA-512`
//! 全文件（`EXACT_DUP`）+ 熵流 hash（`JPEG` SOS→EOF / `PNG` IDAT / `ISO-BMFF` mdat，
//! 剥元数据只比像素流 → `PIXEL_SAME`）+ 旋转校正 pHash（四向 min hamming →
//! `ROTATED_SAME`）。

use camino::Utf8PathBuf;
use sha2::{Digest, Sha512};

use crate::entities::SecureHash;
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::copy::Source;
use crate::usecases::cull::phash::{hamming, phash};

/// 对单个源文件裁定内容比对 verdict（`exact_dup`/`pixel_same`/`rotated_same`/
/// `name_only`/`absent`）。读失败或超 `max_bytes` 的文件降级 `name_only`（有同名
/// 候选）或 `absent`（无候选）。
pub(crate) fn verdict_for(info: &Info, out: &Index, phash_max: u8, max_bytes: u64) -> String {
    if matches!(out.exists(info, true), Ok(Some(_))) {
        return "exact_dup".to_owned();
    }
    let base = info
        .full_path
        .file_name()
        .map(str::to_owned)
        .unwrap_or_default();
    let cands = find_candidates(out, &base);
    if cands.is_empty() {
        return "absent".to_owned();
    }
    let Some(src_bytes) = read_guarded(info, max_bytes) else {
        return "name_only".to_owned();
    };
    let ext = info
        .full_path
        .extension()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
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

/// 构建输出库索引（output 非目录时为空，验证未归档场景）。
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

fn find_candidates(out: &Index, base: &str) -> Vec<Utf8PathBuf> {
    let base_lower = base.to_ascii_lowercase();
    let (stem, ext) = split_stem_ext(&base_lower);
    out.iter()
        .filter_map(|entry| {
            let name = basename(entry.key().as_str());
            let name_lower = name.to_ascii_lowercase();
            if name_lower == base_lower {
                return Some(entry.key().clone());
            }
            let (nstem, next) = split_stem_ext(&name_lower);
            if next == ext && stem_digit_variant(&nstem, &stem) {
                Some(entry.key().clone())
            } else {
                None
            }
        })
        .collect()
}

fn split_stem_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) => (name[..i].to_owned(), name[i..].to_owned()),
        None => (name.to_owned(), String::new()),
    }
}

fn stem_digit_variant(name_stem: &str, base_stem: &str) -> bool {
    let Some(digits) = name_stem
        .strip_prefix(base_stem)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return false;
    };
    if digits.is_empty() {
        return false;
    }
    digits.chars().all(|c| c.is_ascii_digit())
}

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn read_guarded(info: &Info, max_bytes: u64) -> Option<Vec<u8>> {
    if info.size > max_bytes {
        return None;
    }
    read_to_end(info.backend().as_ref(), info.location(), info.size)
}

fn read_guarded_by_path(out: &Index, path: &Utf8PathBuf, max_bytes: u64) -> Option<Vec<u8>> {
    let info = out.get(path)?;
    read_guarded(&info, max_bytes)
}

fn read_to_end(backend: &dyn Backend, loc: &Location, size: u64) -> Option<Vec<u8>> {
    let mut r = backend.open_read(loc).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    std::io::Read::read_to_end(&mut r, &mut buf).ok()?;
    Some(buf)
}

/// 全文件 SHA-512（内容比对单元测试辅助；生产 `EXACT_DUP` 走 `Index::exists(secure=true)`）。
#[cfg(test)]
#[must_use]
pub(crate) fn sha512_bytes(bytes: &[u8]) -> SecureHash {
    let mut h = Sha512::new();
    h.update(bytes);
    h.finalize()
}

/// 按扩展名算「像素流熵」哈希：仅 JPEG SOS 之后 / PNG IDAT 拼接 / BMFF mdat payload，
/// 忽略元数据差异。不支持或解析失败返 `None`。
#[doc(hidden)]
#[must_use]
pub fn entropy_hash(bytes: &[u8], ext: &str) -> Option<SecureHash> {
    match ext {
        "jpg" | "jpeg" => jpeg_entropy_hash(bytes).or_else(|| png_idat_hash(bytes)),
        "png" => png_idat_hash(bytes).or_else(|| jpeg_entropy_hash(bytes)),
        "mp4" | "mov" | "m4v" | "3gp" | "heic" | "heif" => bmff_mdat_hash(bytes),
        _ => None,
    }
}

/// 旋转校正 pHash 相似：源图 4 向（0/90/180/270）与目标图比较，任一方向 Hamming
/// ≤ `max_hamming` 且尺寸（允许旋转交换宽高）一致 → 相似。解码失败返 `false`。
#[doc(hidden)]
#[must_use]
pub fn rotated_phash_similar(a: &[u8], b: &[u8], max_hamming: u8) -> bool {
    let (Ok(img_a), Ok(img_b)) = (image::load_from_memory(a), image::load_from_memory(b)) else {
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
    let a90 = image::imageops::rotate90(&a_rgb);
    let a180 = image::imageops::rotate180(&a_rgb);
    let a270 = image::imageops::rotate270(&a_rgb);
    [&a_rgb, &a90, &a180, &a270]
        .iter()
        .any(|img| hamming(phash(img), h_b) <= u32::from(max_hamming))
}

#[doc(hidden)]
#[must_use]
pub fn jpeg_entropy_hash(bytes: &[u8]) -> Option<SecureHash> {
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
        // 无长度字段的 marker：SOI/RES 段（TEM、RST 范围）继续扫。
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i = j + 1;
            continue;
        }
        if j + 2 > bytes.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([bytes[j + 1], bytes[j + 2]]) as usize;
        // 段（含 marker 与 len 字段自身）总长 = seg_len + 2；下一 marker 在
        // j + seg_len + 1（j 是 marker code 位置，FF 在其前一字节）。
        if marker == 0xDA {
            return Some(hash_rest(bytes, j + 1 + seg_len));
        }
        i = j + 1 + seg_len;
    }
    None
}

#[doc(hidden)]
#[must_use]
pub fn png_idat_hash(bytes: &[u8]) -> Option<SecureHash> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIG) {
        return None;
    }
    let mut h = Sha512::new();
    let mut found = false;
    let mut pos = SIG.len();
    while pos + 8 <= bytes.len() {
        let ln = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
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

#[doc(hidden)]
#[must_use]
pub fn bmff_mdat_hash(bytes: &[u8]) -> Option<SecureHash> {
    let mut h = Sha512::new();
    let mut found = false;
    let mut pos = 0;
    while pos + 8 <= bytes.len() {
        let ln32 = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?);
        let typ = &bytes[pos + 4..pos + 8];
        let (ln, hdr_len) = match ln32 {
            1 => {
                let wide = u64::from_be_bytes(bytes.get(pos + 8..pos + 16)?.try_into().ok()?);
                (wide, 16)
            }
            0 => (u64::try_from(bytes.len() - pos).ok()?, 8),
            n => (u64::from(n), 8),
        };
        if ln < 8 {
            break;
        }
        if typ == b"mdat" {
            found = true;
            let start = pos + hdr_len;
            let end = pos
                .saturating_add(usize::try_from(ln).ok()?)
                .min(bytes.len());
            h.update(bytes.get(start..end)?);
        }
        pos = pos.saturating_add(usize::try_from(ln).ok()?);
    }
    found.then(|| h.finalize())
}

#[doc(hidden)]
#[must_use]
pub fn hash_rest(bytes: &[u8], from: usize) -> SecureHash {
    let mut h = Sha512::new();
    h.update(bytes.get(from..).unwrap_or(&[]));
    h.finalize()
}

#[cfg(test)]
#[path = "content_diff_tests.rs"]
mod tests;
