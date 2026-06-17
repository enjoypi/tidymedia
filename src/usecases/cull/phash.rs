//! 感知哈希（Average Hash 8×8 灰度均值）+ 汉明距离 + Union-Find 相似分组。
//!
//! 选 Average Hash 而非 DCT pHash：实现 30 行（DCT 需自写 2D 离散变换 ~80 行），
//! 对连拍/微调照片的判同语义足够；HDR bracket 系列对全局亮度漂移仍能识别（汉明 ≤ 5）。
//! 扩展点：未来需更鲁棒可换 DCT pHash（接口不变）。
//!
//! Union-Find O(N²·α) 在 N < 500 连拍场景完全可忽略。

/// 输入 RGB → 8×8 灰度 → 与均值比对得 64-bit hash。
#[must_use]
pub(crate) fn ahash(img: &image::RgbImage) -> u64 {
    let small = image::imageops::resize(img, 8, 8, image::imageops::FilterType::Triangle);
    let luma = image::imageops::grayscale(&image::DynamicImage::ImageRgb8(small));
    let mut sum: u32 = 0;
    for px in luma.pixels() {
        sum += u32::from(px.0[0]);
    }
    let mean = sum / 64;
    let mut hash: u64 = 0;
    for (i, px) in luma.pixels().enumerate() {
        if u32::from(px.0[0]) >= mean {
            hash |= 1_u64 << i;
        }
    }
    hash
}

#[must_use]
pub(crate) fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 按汉明距离 ≤ `max_hamming` 把入参 hash 分组。Union-Find 传递闭包。
/// 返回每组在原 slice 中的索引列表，按组首索引升序。
#[must_use]
pub(crate) fn group_by_hash(hashes: &[u64], max_hamming: u8) -> Vec<Vec<usize>> {
    use std::collections::BTreeMap;

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

    fn solid(color: [u8; 3]) -> image::RgbImage {
        image::RgbImage::from_pixel(16, 16, image::Rgb(color))
    }

    #[test]
    fn identical_images_have_same_hash() {
        let a = solid([100, 100, 100]);
        let b = solid([100, 100, 100]);
        assert_eq!(ahash(&a), ahash(&b));
    }

    #[test]
    fn ahash_mixed_pixels_yield_below_mean_bits() {
        // 半亮半暗 → resize 后 8×8 含 < mean 的像素，命中 line 21 false 分支
        let mut img = image::RgbImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let v = if x < 8 { 0 } else { 255 };
                img.put_pixel(x, y, image::Rgb([v, v, v]));
            }
        }
        let h = ahash(&img);
        // 既有 1 位也有 0 位 → 既不是全 0 也不是全 1
        assert_ne!(h, 0);
        assert_ne!(h, u64::MAX);
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
    fn group_by_hash_unions_close_pairs() {
        // 三个 hash：0b0000, 0b0001 (hamming 1), 0b1111 (hamming 3 vs 0b0000)
        let hashes = vec![0_u64, 1, 0x0F];
        let g = group_by_hash(&hashes, 1);
        // 0 与 1 同组（汉明 1），0x0F 独立（与 0 汉明 4）
        assert_eq!(g.len(), 2);
        let sizes: Vec<usize> = g.iter().map(Vec::len).collect();
        assert!(sizes.contains(&2), "{sizes:?}");
        assert!(sizes.contains(&1), "{sizes:?}");
    }

    #[test]
    fn group_by_hash_transitive_closure() {
        // A-B (hamming 1), B-C (hamming 1) → 三个全归一组（即使 A-C hamming 2 > 阈值）
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
        // 4 个 hash 两两 hamming ≤ 2 → 全连通 → 多次 union 同组触发 ra == rb 分支
        let hashes = vec![0b00_u64, 0b01, 0b10, 0b11];
        let g = group_by_hash(&hashes, 2);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 4);
    }
}
