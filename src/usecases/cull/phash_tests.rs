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

/// 渐变图：DCT 低频集中、对缩放鲁棒，作为 downscale 稳定性测试 fixture。
fn gradient() -> RgbImage {
    gradient_at(64)
}

/// 任意分辨率渐变图。≥ 256 × 256 大尺寸用于稳定性测试（与 CLAUDE.md
/// 「pHash 测试 fixture 设计」一致：低分辨率 + checker 经 phash 32 × 32 缩放
/// 高频损失严重，不适合做缩放/重压缩稳定性断言）。
fn gradient_at(side: u32) -> RgbImage {
    let mut img = RgbImage::new(side, side);
    for y in 0_u32..side {
        for x in 0_u32..side {
            // 线性映射 0..255 区间，避免 `% 255` 取模在大尺寸下出现「断崖」让 phash
            // 把图像识别为多个 stripe pattern 而虚增汉明距离。
            let v = u8::try_from(((x + y) * 255 / (2 * side - 2).max(1)).min(255))
                .expect("internal: clamped to <=255 fits u8");
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
    // 256×256 high-entropy random fill + 像素 ±5 平移 → DCT 系数远离中位数 →
    // 小幅 brightness 偏移翻转的低频系数符号极少 → Hamming 距离应很小。
    // 旧实现用 64×64 checker：经 phash 32×32 缩放后高频损失严重让断言脆弱
    // （CLAUDE.md「pHash 测试 fixture 设计」明确大图 + random/gradient 才是
    // 稳定性测试口径，本测试用 random 而非纯 gradient 以避开后者中位数附近
    // 系数密集的脆弱性）。
    let mut a = RgbImage::new(256, 256);
    for (i, px) in a.pixels_mut().enumerate() {
        // 高熵 noise：与 cull/run_tests.rs::write_random_png 同套路
        let v =
            u8::try_from((i.wrapping_mul(37) ^ (i >> 3)) & 0xff).expect("internal: & 0xff < 256");
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
    // 同 gradient 公式以 128×128 与 64×64 两分辨率生成 → phash 内部都缩 32 →
    // 低频系数与中位数相对关系保持，Hamming 应较小。
    let mut big = RgbImage::new(128, 128);
    for y in 0_u32..128 {
        for x in 0_u32..128 {
            let v = u8::try_from((x + y) % 255).expect("internal: mod 255 fits u8");
            big.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let small = gradient();
    let d = hamming(phash(&big), phash(&small));
    assert!(d <= 20, "Hamming {d} > 20");
}

#[test]
fn phash_stable_under_jpeg_recompression() {
    // 256×256 gradient JPEG 重压缩（质量 90）→ DCT 低频系数几乎不变 → Hamming 小。
    // 小尺寸（≤64）+ 低质量 JPEG 会让 phash 中位数附近系数翻转较多，故用大图+高质量
    // 模拟典型相机原图重存场景。
    use image::codecs::jpeg::JpegEncoder;
    let mut a = RgbImage::new(256, 256);
    for y in 0_u32..256 {
        for x in 0_u32..256 {
            let v = u8::try_from((x + y) % 256).expect("internal: mod 256 fits u8");
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
    // 全黑 vs 全白 vs checker 三者 hash 差异显著
    let dark = solid([0, 0, 0]);
    let light = solid([255, 255, 255]);
    let ck = checker();
    // 全黑全白经 phash 中位数阈值后可能 hash 接近（DC 之外低频全 0 → 中位数 0）；
    // checker 与两者结构差异大，至少 Hamming > 5。
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
    // 常数图 → DCT 输出只有 (0,0) DC 显著，其他系数 ≈ 0
    let input = [[42.0_f32; DCT_SIDE]; DCT_SIDE];
    let out = dct_2d(&input);
    let dc = out[0][0].abs();
    // 抽查几个非 DC 系数 ≈ 0
    for &(u, v) in &[(0_usize, 1_usize), (1, 0), (5, 7), (15, 23)] {
        let val = out[u][v];
        assert!(val.abs() < dc * 1e-3, "(u,v)=({u},{v}) value={val} dc={dc}");
    }
}

#[test]
fn group_by_hash_unions_close_pairs() {
    let hashes = vec![0_u64, 1, 0x0F];
    let g = group_by_hash(&hashes, 1);
    assert_eq!(g.len(), 2);
    let sizes: Vec<usize> = g.iter().map(Vec::len).collect();
    assert!(sizes.contains(&2), "{sizes:?}");
    assert!(sizes.contains(&1), "{sizes:?}");
}

#[test]
fn group_by_hash_transitive_closure() {
    let hashes = vec![0b00_u64, 0b01, 0b11];
    let g = group_by_hash(&hashes, 1);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].len(), 3);
}

#[test]
fn group_by_hash_empty_input() {
    assert!(group_by_hash(&[], 5).is_empty());
}

#[test]
fn group_by_hash_single_input() {
    let g = group_by_hash(&[42], 5);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0], vec![0]);
}

#[test]
fn group_by_hash_redundant_union_hits_same_root() {
    let hashes = vec![0b00_u64, 0b01, 0b10, 0b11];
    let g = group_by_hash(&hashes, 2);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].len(), 4);
}
