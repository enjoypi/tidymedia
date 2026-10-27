use std::sync::OnceLock;

use image::RgbImage;

const DCT_SIDE: usize = 32;
const DCT_SIDE_U32: u32 = 32;
const HASH_SIDE: usize = 8;

#[must_use]
pub(crate) fn phash(img: &RgbImage) -> u64 {
    let small = super::content_diff_io::resize(img, DCT_SIDE_U32);
    let luma = super::content_diff_io::grayscale(small);
    let mut pixels = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
    for (y, row) in pixels.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let xu = u32::try_from(x).expect("internal: x < DCT_SIDE fits u32");
            let yu = u32::try_from(y).expect("internal: y < DCT_SIDE fits u32");
            let px = luma.get_pixel(xu, yu);
            *cell = f32::from(px.0[0]);
        }
    }
    let dct = dct_2d_low(&pixels);
    hash_from_block(&dct)
}

fn hash_from_block(dct: &[[f32; HASH_SIDE]; HASH_SIDE]) -> u64 {
    let mut block = [0.0_f32; HASH_SIDE * HASH_SIDE];
    for (u, row) in dct.iter().enumerate() {
        block[u * HASH_SIDE..(u + 1) * HASH_SIDE].copy_from_slice(row);
    }
    let mut without_dc: Vec<f32> = block[1..].to_vec();
    without_dc.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = without_dc[without_dc.len() / 2];
    if median.abs() < f32::EPSILON {
        return 0;
    }
    let mut hash = 0_u64;
    for (i, &v) in block.iter().enumerate() {
        if v >= median {
            hash |= 1_u64 << i;
        }
    }
    hash
}

fn dct_2d_low(input: &[[f32; DCT_SIDE]; DCT_SIDE]) -> [[f32; HASH_SIDE]; HASH_SIDE] {
    let cos = cos_table();
    let mut tmp = [[0.0_f32; HASH_SIDE]; DCT_SIDE];
    for (y, row_in) in input.iter().enumerate() {
        for (u, tmp_cell) in tmp[y].iter_mut().enumerate() {
            let mut s = 0.0_f32;
            for (x, &v) in row_in.iter().enumerate() {
                s = v.mul_add(cos[u][x], s);
            }
            *tmp_cell = s;
        }
    }
    let mut out = [[0.0_f32; HASH_SIDE]; HASH_SIDE];
    for (v, row) in out.iter_mut().enumerate() {
        for (u, cell) in row.iter_mut().enumerate() {
            let mut s = 0.0_f32;
            for y in 0..DCT_SIDE {
                s = tmp[y][u].mul_add(cos[v][y], s);
            }
            *cell = s;
        }
    }
    out
}

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

#[cfg(test)]
#[path = "phash_tests.rs"]
mod tests;
