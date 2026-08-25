use std::io::Cursor;

use image::RgbImage;

use super::{entropy_hash, rotated_phash_similar, sha512_bytes, stem_digit_variant};

/// `photo_2021_12.jpg` 类：`basename` 剥掉 `base_stem` + `_` 后是纯数字后缀
/// （重复导出序号）→ 视为同内容变体。
#[test]
fn stem_digit_variant_detects_numeric_suffix() {
    assert!(stem_digit_variant("photo_2021_12", "photo_2021"));
    assert!(stem_digit_variant("IMG_20210501_3", "IMG_20210501"));
}

#[test]
fn stem_digit_variant_rejects_non_digit_or_missing() {
    assert!(!stem_digit_variant("photo_2021_abc", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021_", "photo_2021"));
    assert!(!stem_digit_variant("other_2021_12", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021_1_x", "photo_2021_1"));
}

fn chunk(typ: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&typ);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

#[test]
fn sha512_is_deterministic() {
    assert_eq!(sha512_bytes(b"abc"), sha512_bytes(b"abc"));
    assert_ne!(sha512_bytes(b"abc"), sha512_bytes(b"abd"));
}

#[test]
fn jpeg_entropy_hash_hashes_only_after_sos() {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
    bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    bytes.extend_from_slice(b"ABCDEF");
    assert_eq!(entropy_hash(&bytes, "jpg"), Some(sha512_bytes(b"ABCDEF")));
}

#[test]
fn jpeg_entropy_hash_rejects_non_jpeg() {
    assert_eq!(entropy_hash(b"GIF89a", "jpg"), None);
}

#[test]
fn png_idat_hash_concats_idat_payload() {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&chunk(*b"IHDR", &[0; 13]));
    bytes.extend_from_slice(&chunk(*b"IDAT", b"PIXELDATA"));
    bytes.extend_from_slice(&chunk(*b"IEND", &[]));
    assert_eq!(
        entropy_hash(&bytes, "png"),
        Some(sha512_bytes(b"PIXELDATA"))
    );
}

#[test]
fn png_idat_hash_concats_multiple_idat() {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&chunk(*b"IHDR", &[0; 13]));
    bytes.extend_from_slice(&chunk(*b"IDAT", b"AB"));
    bytes.extend_from_slice(&chunk(*b"IDAT", b"CD"));
    bytes.extend_from_slice(&chunk(*b"IEND", &[]));
    assert_eq!(entropy_hash(&bytes, "png"), Some(sha512_bytes(b"ABCD")));
}

fn bmff_box(typ: [u8; 4], data: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + data.len()).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&typ);
    out.extend_from_slice(data);
    out
}

#[test]
fn bmff_mdat_hash_32bit_size() {
    let mut bytes = bmff_box(*b"ftyp", b"M4A ");
    bytes.extend_from_slice(&bmff_box(*b"mdat", b"MMMDAT"));
    assert_eq!(entropy_hash(&bytes, "mp4"), Some(sha512_bytes(b"MMMDAT")));
}

#[test]
fn bmff_mdat_hash_64bit_size() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(1_u32).to_be_bytes());
    bytes.extend_from_slice(b"mdat");
    bytes.extend_from_slice(&(23_u64).to_be_bytes());
    bytes.extend_from_slice(b"WIDEDAT");
    assert_eq!(entropy_hash(&bytes, "mp4"), Some(sha512_bytes(b"WIDEDAT")));
}

#[test]
fn bmff_mdat_hash_rest_of_file() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(0_u32).to_be_bytes());
    bytes.extend_from_slice(b"mdat");
    bytes.extend_from_slice(b"EVERYTHING");
    assert_eq!(
        entropy_hash(&bytes, "mp4"),
        Some(sha512_bytes(b"EVERYTHING"))
    );
}

fn noise_image(side: u32, seed: u64) -> RgbImage {
    let mut img = RgbImage::new(side, side);
    for (i, px) in img.pixels_mut().enumerate() {
        let v = u8::try_from(((i as u64).wrapping_mul(seed) ^ (i as u64 >> 3)) & 0xff)
            .expect("internal: & 0xff < 256");
        px.0 = [v, v, v];
    }
    img
}

fn encode_png(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone())
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("internal: png encode");
    buf
}

#[test]
fn rotated_phash_detects_rotated_copy() {
    let a = noise_image(128, 37);
    let rotated = image::imageops::rotate90(&a);
    let b = noise_image(128, 53);
    assert!(
        rotated_phash_similar(&encode_png(&a), &encode_png(&rotated), 10),
        "rotated copy should match"
    );
    assert!(
        !rotated_phash_similar(&encode_png(&a), &encode_png(&b), 10),
        "unrelated images must not match"
    );
}

#[test]
fn rotated_phash_decoding_failure_returns_false() {
    assert!(!rotated_phash_similar(b"not-an-image", b"also-not", 10));
}
