//! 感知哈希（DCT pHash 32×32 → 8×8 低频 → 中位数阈值 64-bit）+ 汉明距离 +
//! Union-Find 相似分组。
//!
//! DCT 比 Average Hash 对 HDR bracket / JPEG 重压缩 / 微小缩放更鲁棒：低频系数
//! 表征图像结构信息，对全局亮度/噪声漂移免疫。算法：
//! 1. RGB 双线性下采样到 32×32，转灰度；
//! 2. 2D `DCT-II`（行+列两次 1D pass，预算 32×32 cos 表加速）；
//! 3. 取左上 8×8 = 64 系数；
//! 4. 中位数取自后 63 个（剔除 (0,0) DC 偏置）；64 元素逐位与中位数比较生成 64-bit hash。
//!
//! Union-Find 与汉明距离接口保持不变（O(N²·α)；N < 500 连拍场景毫秒级）。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use image::{DynamicImage, RgbImage};

const DCT_SIDE: usize = 32;
const DCT_SIDE_U32: u32 = 32;
const HASH_SIDE: usize = 8;

/// 计算 `img` 的 `DCT` pHash。
#[must_use]
pub(crate) fn phash(img: &RgbImage) -> u64 {
    let small = image::imageops::resize(
        img,
        DCT_SIDE_U32,
        DCT_SIDE_U32,
        image::imageops::FilterType::Triangle,
    );
    let luma = image::imageops::grayscale(&DynamicImage::ImageRgb8(small));
    let mut pixels = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
    for (y, row) in pixels.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let xu = u32::try_from(x).expect("internal: x < DCT_SIDE fits u32");
            let yu = u32::try_from(y).expect("internal: y < DCT_SIDE fits u32");
            let px = luma.get_pixel(xu, yu);
            *cell = f32::from(px.0[0]);
        }
    }
    let dct = dct_2d(&pixels);
    hash_from_block(&dct)
}

/// 把 32×32 `DCT` 结果的左上 8×8 块按中位数（剔除 DC）阈值化成 64-bit hash。
fn hash_from_block(dct: &[[f32; DCT_SIDE]; DCT_SIDE]) -> u64 {
    let mut block = [0.0_f32; HASH_SIDE * HASH_SIDE];
    for u in 0..HASH_SIDE {
        for v in 0..HASH_SIDE {
            block[u * HASH_SIDE + v] = dct[u][v];
        }
    }
    let mut without_dc: Vec<f32> = block[1..].to_vec();
    without_dc.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = without_dc[without_dc.len() / 2];
    let mut hash: u64 = 0;
    for (i, &v) in block.iter().enumerate() {
        if v >= median {
            hash |= 1_u64 << i;
        }
    }
    hash
}

/// 2D `DCT-II`：行 pass + 列 pass，cos 表懒初始化。
fn dct_2d(input: &[[f32; DCT_SIDE]; DCT_SIDE]) -> [[f32; DCT_SIDE]; DCT_SIDE] {
    let cos = cos_table();
    // row pass：tmp[y][u] = Σ_x input[y][x] · cos[u][x]
    let mut tmp = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
    for (y, row_in) in input.iter().enumerate() {
        for (u, tmp_cell) in tmp[y].iter_mut().enumerate() {
            let mut s = 0.0_f32;
            for (x, &px) in row_in.iter().enumerate() {
                s = px.mul_add(cos[u][x], s);
            }
            *tmp_cell = s;
        }
    }
    // col pass：out[v][u] = Σ_y tmp[y][u] · cos[v][y]
    let mut out = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
    for v in 0..DCT_SIDE {
        for u in 0..DCT_SIDE {
            let mut s = 0.0_f32;
            for y in 0..DCT_SIDE {
                s = tmp[y][u].mul_add(cos[v][y], s);
            }
            out[v][u] = s;
        }
    }
    out
}

/// `cos_table[i][k] = cos((2k+1)·i·π/(2N))`，N=32。一次性懒初始化共享。
fn cos_table() -> &'static [[f32; DCT_SIDE]; DCT_SIDE] {
    static TABLE: OnceLock<[[f32; DCT_SIDE]; DCT_SIDE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
        #[expect(
            clippy::cast_precision_loss,
            reason = "i/k < 32 远小于 f32 mantissa 24-bit 精度边界"
        )]
        let n_f = (DCT_SIDE * 2) as f32;
        for (i, row) in t.iter_mut().enumerate() {
            for (k, cell) in row.iter_mut().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "i/k < 32 远小于 f32 mantissa 精度边界"
                )]
                let arg = ((2 * k + 1) as f32) * (i as f32) * std::f32::consts::PI / n_f;
                *cell = arg.cos();
            }
        }
        t
    })
}

#[must_use]
pub(crate) fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 按汉明距离 ≤ `max_hamming` 把入参 hash 分组。Union-Find 传递闭包。
/// 返回每组在原 slice 中的索引列表，按组首索引升序。
#[must_use]
pub(crate) fn group_by_hash(hashes: &[u64], max_hamming: u8) -> Vec<Vec<usize>> {
    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] == i {
            return i;
        }
        let r = find(parent, parent[i]);
        parent[i] = r;
        r
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    let n = hashes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(hashes[i], hashes[j]) <= u32::from(max_hamming) {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
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
        let mut img = RgbImage::new(64, 64);
        for y in 0_u32..64 {
            for x in 0_u32..64 {
                let v = u8::try_from((x + y) * 2 % 255).expect("internal: mod 255 fits u8");
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
        // checker 像素 ±5 平移 → DCT 低频结构保持 → Hamming 距离很小
        let a = checker();
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
            assert!(
                out[u][v].abs() < dc * 1e-3,
                "(u,v)=({u},{v}) value={} dc={dc}",
                out[u][v]
            );
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
}
