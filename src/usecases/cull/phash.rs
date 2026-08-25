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
    // 常数信号（全黑/全白/纯色图）所有 AC 高频系数 ≈ 0 → median ≈ 0 → 全部
    // `v >= 0` 命中致 hash=u64::MAX，两张完全不同的纯色图碰撞为同 hash。
    // median ≈ 0 时短路返 0：让纯色图都映射到独立桶（u64=0）外的「无效 hash」
    // 即 0，与正常含纹理图（hash 几乎不可能为 0）冲突概率最小。
    if median.abs() < f32::EPSILON {
        return 0;
    }
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
#[path = "phash_tests.rs"]
mod tests;
