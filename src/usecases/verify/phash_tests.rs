use super::*;

fn solid(color: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(64, 64, image::Rgb(color))
}

fn checker() -> RgbImage {
    let mut img = RgbImage::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let v = if (x / 8 + y / 8) % 2 == 0 { 30 } else { 220 };
            img.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    img
}

fn gradient() -> RgbImage {
    let mut img = RgbImage::new(64, 64);
    for y in 0_u32..64 {
        for x in 0_u32..64 {
            let v = u8::try_from(((x + y) * 255 / 126).min(255)).unwrap();
            img.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    img
}

#[test]
fn phash_identical_images_have_same_hash() {
    let a = solid([100, 100, 100]);
    let b = solid([100, 100, 100]);
    assert_eq!(phash(&a), phash(&b));
}

#[test]
fn phash_mixed_pixels_yield_both_zero_and_one_bits() {
    let h = phash(&checker());
    assert_ne!(h, 0);
    assert_ne!(h, u64::MAX);
}

#[test]
fn phash_stable_under_minor_brightness_shift() {
    let mut a = RgbImage::new(256, 256);
    for (i, px) in a.pixels_mut().enumerate() {
        let v = u8::try_from((i.wrapping_mul(37) ^ (i >> 3)) & 0xff).unwrap();
        px.0 = [v, v, v];
    }
    let mut b = a.clone();
    for px in b.pixels_mut() {
        for ch in &mut px.0 {
            *ch = ch.saturating_add(5);
        }
    }
    let d = hamming(phash(&a), phash(&b));
    assert!(d <= 8, "Hamming {d} > 8");
}

#[test]
fn phash_stable_across_input_resolution() {
    let mut big = RgbImage::new(128, 128);
    for y in 0_u32..128 {
        for x in 0_u32..128 {
            let v = u8::try_from((x + y) % 255).unwrap();
            big.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let small = gradient();
    let d = hamming(phash(&big), phash(&small));
    assert!(d <= 20, "Hamming {d} > 20");
}

#[test]
fn phash_stable_under_jpeg_recompression() {
    use image::codecs::jpeg::JpegEncoder;
    let mut a = RgbImage::new(256, 256);
    for y in 0_u32..256 {
        for x in 0_u32..256 {
            let v = u8::try_from((x + y) % 256).unwrap();
            a.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let mut buf = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 90);
    encoder
        .encode(
            a.as_raw(),
            a.width(),
            a.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let recompressed = image::load_from_memory(&buf).unwrap().to_rgb8();
    let d = hamming(phash(&a), phash(&recompressed));
    assert!(d <= 12, "Hamming {d} > 12");
}

#[test]
fn phash_distinguishes_unrelated_images() {
    let dark = solid([0, 0, 0]);
    let light = solid([255, 255, 255]);
    let ck = checker();
    let d1 = hamming(phash(&dark), phash(&ck));
    let d2 = hamming(phash(&light), phash(&ck));
    assert!(d1.max(d2) > 5, "d1={d1} d2={d2}");
}

#[test]
fn hamming_zero_for_equal() {
    assert_eq!(hamming(0xDEAD_BEEF_BAAD_F00D, 0xDEAD_BEEF_BAAD_F00D), 0);
}

#[test]
fn hamming_counts_bit_diffs() {
    assert_eq!(hamming(0b1010, 0b0101), 4);
}

#[test]
fn dct_2d_of_constant_concentrates_energy_in_dc() {
    let input = [[42.0_f32; DCT_SIDE]; DCT_SIDE];
    let out = dct_2d(&input);
    let dc = out[0][0].abs();
    for &(u, v) in &[(0_usize, 1_usize), (1, 0), (5, 7), (15, 23)] {
        let val = out[u][v];
        assert!(val.abs() < dc * 1e-3, "(u,v)=({u},{v}) value={val} dc={dc}");
    }
}
