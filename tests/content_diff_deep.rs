use std::io::Cursor;

use image::RgbImage;
use sha2::{Digest, Sha512};
use tidymedia::{
    bmff_mdat_hash, entropy_hash, hash_rest, jpeg_entropy_hash, png_idat_hash,
    rotated_phash_similar,
};

#[test]
fn jpeg_rejects_short_or_wrong_magic() {
    assert_eq!(jpeg_entropy_hash(&[0xFF, 0xD8]), None);
    assert_eq!(jpeg_entropy_hash(&[0x00, 0xFF, 0xD8, 0xFF]), None);
    assert_eq!(jpeg_entropy_hash(&[0xFF, 0x00, 0x00, 0x00]), None);
}

#[test]
fn jpeg_hashes_payload_after_sos() {
    let mut b = vec![0xFF, 0xD8];
    b.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    b.extend_from_slice(b"ABCDEF");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"ABCDEF")));
}

#[test]
fn jpeg_skips_non_marker_bytes() {
    let mut b = vec![0xFF, 0xD8, 0x41];
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(b"AB");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"AB")));
}

#[test]
fn jpeg_skips_run_of_fill_bytes() {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xFF, 0xFF];
    b.extend_from_slice(&[0xDA, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(b"XY");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"XY")));
}

#[test]
fn jpeg_returns_none_when_ff_run_hits_end() {
    assert_eq!(
        jpeg_entropy_hash(&[0xFF, 0xD8, 0xFF, 0xFF, 0xFF, 0xFF]),
        None
    );
}

#[test]
fn jpeg_returns_none_when_no_sos_marker() {
    assert_eq!(jpeg_entropy_hash(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
}

#[test]
fn jpeg_skips_no_length_markers() {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xD8];
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(b"P");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"P")));
    let mut b = vec![0xFF, 0xD8, 0xFF, 0x01];
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(b"Q");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"Q")));
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xD3];
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x00, 0x00]);
    b.extend_from_slice(b"R");
    assert_eq!(jpeg_entropy_hash(&b), Some(Sha512::digest(b"R")));
}

#[test]
fn jpeg_returns_none_when_len_field_truncated() {
    // 尾 marker 的 2 字节长度字段越界（j+2 > len）→ None
    assert_eq!(
        jpeg_entropy_hash(&[0xFF, 0xD8, 0xFF, 0xFF, 0xFF, 0xE0]),
        None
    );
}

fn chunk(typ: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&typ);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

#[test]
fn png_rejects_wrong_magic() {
    assert_eq!(png_idat_hash(b"not-png-bytes"), None);
}

#[test]
fn png_returns_none_without_idat() {
    let mut bytes = SIG.to_vec();
    bytes.extend_from_slice(&chunk(*b"IHDR", &[0; 13]));
    assert_eq!(png_idat_hash(&bytes), None);
}

#[test]
fn png_hashes_concatenated_idat() {
    let mut bytes = SIG.to_vec();
    bytes.extend_from_slice(&chunk(*b"IHDR", &[0; 13]));
    bytes.extend_from_slice(&chunk(*b"IDAT", b"AB"));
    bytes.extend_from_slice(&chunk(*b"IDAT", b"CD"));
    bytes.extend_from_slice(&chunk(*b"IEND", &[]));
    assert_eq!(png_idat_hash(&bytes), Some(Sha512::digest(b"ABCD")));
}

#[test]
fn png_returns_none_when_idat_len_exceeds_file() {
    let mut bytes = SIG.to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 100]);
    bytes.extend_from_slice(b"IDAT");
    bytes.extend_from_slice(b"abc");
    assert_eq!(png_idat_hash(&bytes), None);
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
fn bmff_returns_none_without_mdat() {
    assert_eq!(bmff_mdat_hash(&bmff_box(*b"ftyp", b"M4A ")), None);
}

#[test]
fn bmff_hashes_32bit_mdat() {
    let mut bytes = bmff_box(*b"ftyp", b"M4A ");
    bytes.extend_from_slice(&bmff_box(*b"mdat", b"MMMDAT"));
    assert_eq!(bmff_mdat_hash(&bytes), Some(Sha512::digest(b"MMMDAT")));
}

#[test]
fn bmff_hashes_64bit_mdat() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(1_u32).to_be_bytes());
    bytes.extend_from_slice(b"mdat");
    bytes.extend_from_slice(&(23_u64).to_be_bytes());
    bytes.extend_from_slice(b"WIDEDAT");
    assert_eq!(bmff_mdat_hash(&bytes), Some(Sha512::digest(b"WIDEDAT")));
}

#[test]
fn bmff_hashes_rest_of_file() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(0_u32).to_be_bytes());
    bytes.extend_from_slice(b"mdat");
    bytes.extend_from_slice(b"EVERYTHING");
    assert_eq!(bmff_mdat_hash(&bytes), Some(Sha512::digest(b"EVERYTHING")));
}

#[test]
fn bmff_breaks_on_tiny_box() {
    let bytes = [0_u8, 0, 0, 4, b'm', b'd', b'a', b't'];
    assert_eq!(bmff_mdat_hash(&bytes), None);
}

#[test]
fn bmff_breaks_when_wide_len_field_truncated() {
    let bytes = [0_u8, 0, 0, 1, b'm', b'd', b'a', b't', 0, 0, 0, 0];
    assert_eq!(bmff_mdat_hash(&bytes), None);
}

#[test]
fn bmff_breaks_when_wide_size_smaller_than_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(1_u32).to_be_bytes());
    bytes.extend_from_slice(b"mdat");
    bytes.extend_from_slice(&(8_u64).to_be_bytes());
    assert_eq!(bmff_mdat_hash(&bytes), None);
}

#[test]
fn entropy_hash_dispatches_by_extension() {
    let mut jpg = vec![0xFF, 0xD8];
    jpg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x00, 0x00]);
    jpg.extend_from_slice(b"J");
    assert_eq!(entropy_hash(&jpg, "jpg"), Some(Sha512::digest(b"J")));
    let mut png = SIG.to_vec();
    png.extend_from_slice(&chunk(*b"IDAT", b"P"));
    assert_eq!(entropy_hash(&png, "png"), Some(Sha512::digest(b"P")));
    assert_eq!(entropy_hash(&png, "jpg"), Some(Sha512::digest(b"P")));
    // IDAT 截断 → png_idat None → or_else 落 jpeg 再返 None（png arm 的 fallback 闭包）
    let mut trunc = SIG.to_vec();
    trunc.extend_from_slice(&[0, 0, 0, 100]);
    trunc.extend_from_slice(b"IDAT");
    trunc.extend_from_slice(b"abc");
    assert_eq!(entropy_hash(&trunc, "png"), None);
    assert_eq!(
        entropy_hash(&bmff_box(*b"mdat", b"M"), "mov"),
        Some(Sha512::digest(b"M"))
    );
    assert_eq!(entropy_hash(b"plain-text", "txt"), None);
}

#[test]
fn hash_rest_from_offset_and_past_end() {
    assert_eq!(hash_rest(b"hello world", 6), Sha512::digest(b"world"));
    assert_eq!(hash_rest(b"hello", 99), Sha512::digest(b""));
}

fn noise_image(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut img = RgbImage::new(w, h);
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
fn rotated_phash_matches_when_rotated_with_swapped_dims() {
    let a = noise_image(128, 64, 37);
    let rotated = image::imageops::rotate90(&a);
    assert!(
        rotated_phash_similar(&encode_png(&a), &encode_png(&rotated), 10),
        "rotated copy with swapped dims should match"
    );
}

#[test]
fn rotated_phash_rejects_size_mismatch() {
    let a = noise_image(128, 64, 37);
    let small = noise_image(32, 16, 53);
    assert!(
        !rotated_phash_similar(&encode_png(&a), &encode_png(&small), 10),
        "unrelated dims must not match"
    );
}

#[test]
fn rotated_phash_rejects_unrelated_same_size() {
    let a = noise_image(128, 128, 37);
    let b = noise_image(128, 128, 53);
    assert!(
        !rotated_phash_similar(&encode_png(&a), &encode_png(&b), 10),
        "unrelated images must not match"
    );
}

#[test]
fn rotated_phash_decoding_failure_returns_false() {
    assert!(!rotated_phash_similar(b"not-an-image", b"also-not", 10));
}
