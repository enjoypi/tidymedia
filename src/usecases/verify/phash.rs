use std::sync::OnceLock;

use image::{DynamicImage, RgbImage};

const DCT_SIDE: usize = 32;
const DCT_SIDE_U32: u32 = 32;
const HASH_SIDE: usize = 8;

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

fn dct_2d(input: &[[f32; DCT_SIDE]; DCT_SIDE]) -> [[f32; DCT_SIDE]; DCT_SIDE] {
    let cos = cos_table();
    let mut tmp = [[0.0_f32; DCT_SIDE]; DCT_SIDE];
    for (y, row_in) in input.iter().enumerate() {
        for (u, tmp_cell) in tmp[y].iter_mut().enumerate() {
            let mut s = 0.0_f32;
            for (x, &v) in row_in.iter().enumerate() {
                s = v.mul_add(cos[u][x], s);
            }
            *tmp_cell = s;
        }
    }
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
