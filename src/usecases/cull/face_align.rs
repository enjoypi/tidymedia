//! `ArcFace` 5 点仿射对齐：把 SCRFD 出的 5 关键点用相似变换（4 DOF：rotation +
//! uniform scale + translation）映射到 `InsightFace` `ArcFace_112×112` 标准模板，
//! 输出 112×112 RGB。
//!
//! 算法：构造 10×4 线性系统（每对点贡献 2 方程），用法方程 `A^T A x = A^T b` 转
//! 4×4 系统 + Gauss-Jordan 消元解出 `(a, b, tx, ty)`；再以反向映射 + 双线性采样
//! 填充输出像素。规范源：`InsightFace` `ArcFace` 训练 / 推理统一预处理流程。

use std::io;

use image::{Rgb, RgbImage};

/// 对齐输出边长（`MobileFaceNet` / `ArcFace` 标准输入 112×112 RGB）。
pub(crate) const ALIGN_SIDE: u32 = 112;

/// `InsightFace` `ArcFace` 5 点模板：左眼 / 右眼 / 鼻尖 / 左嘴角 / 右嘴角。
const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// 4-DOF 相似变换 `T(x, y) = (a*x - b*y + tx, b*x + a*y + ty)`。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Similarity {
    pub a: f32,
    pub b: f32,
    pub tx: f32,
    pub ty: f32,
}

/// 把 `image` 按 `landmarks` 与模板对齐，输出 `ALIGN_SIDE × ALIGN_SIDE` RGB。
///
/// # Errors
///
/// landmarks 含 NaN/Inf、或法方程矩阵奇异（5 点退化共线/重合）时返 `InvalidInput`。
pub(crate) fn align_face(image: &RgbImage, landmarks: &[[f32; 2]; 5]) -> io::Result<RgbImage> {
    for pt in landmarks {
        if !pt[0].is_finite() || !pt[1].is_finite() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "face_align: landmarks must be finite",
            ));
        }
    }
    let sim = fit_similarity(landmarks)?;
    Ok(warp_inverse(image, &sim, ALIGN_SIDE))
}

/// 5 对点 → 10 方程 → 4 未知数。`(A^T A)·x = A^T b` 是 4×4 系统。
fn fit_similarity(src: &[[f32; 2]; 5]) -> io::Result<Similarity> {
    let mut ata = [[0.0_f32; 4]; 4];
    let mut atb = [0.0_f32; 4];
    for (s, d) in src.iter().zip(TEMPLATE.iter()) {
        let (sx, sy) = (s[0], s[1]);
        let (dx, dy) = (d[0], d[1]);
        // 第 i 个源点贡献两行：
        //   row0 = [sx, -sy, 1, 0]  b = dx
        //   row1 = [sy,  sx, 0, 1]  b = dy
        let r0 = [sx, -sy, 1.0_f32, 0.0_f32];
        let r1 = [sy, sx, 0.0_f32, 1.0_f32];
        accumulate_normal_eq(&mut ata, &mut atb, &r0, dx);
        accumulate_normal_eq(&mut ata, &mut atb, &r1, dy);
    }
    let x = solve_4x4(ata, atb)?;
    Ok(Similarity {
        a: x[0],
        b: x[1],
        tx: x[2],
        ty: x[3],
    })
}

/// `ata += row·rowᵀ; atb += row·target`。法方程逐行累加，保持 `fit_similarity` 主体清爽。
fn accumulate_normal_eq(ata: &mut [[f32; 4]; 4], atb: &mut [f32; 4], row: &[f32; 4], target: f32) {
    for r in 0..4 {
        for c in 0..4 {
            ata[r][c] += row[r] * row[c];
        }
        atb[r] += row[r] * target;
    }
}

/// Gauss-Jordan 消元解 4×4 系统，列主元 partial pivoting。
///
/// # Errors
///
/// 矩阵奇异（任一列主元绝对值 < `SINGULAR_EPS`）时返 `InvalidInput`。
fn solve_4x4(mut a: [[f32; 4]; 4], mut b: [f32; 4]) -> io::Result<[f32; 4]> {
    const SINGULAR_EPS: f32 = 1e-9;
    for i in 0..4 {
        let mut pivot = i;
        let mut best = a[i][i].abs();
        for (k, row) in a.iter().enumerate().take(4).skip(i + 1) {
            if row[i].abs() > best {
                best = row[i].abs();
                pivot = k;
            }
        }
        if best < SINGULAR_EPS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "face_align: singular matrix in similarity fit",
            ));
        }
        if pivot != i {
            a.swap(i, pivot);
            b.swap(i, pivot);
        }
        let inv = 1.0 / a[i][i];
        for cell in &mut a[i][i..] {
            *cell *= inv;
        }
        b[i] *= inv;
        // 复制 pivot 行（i 行）以解开对 a 的双重借用：消去 r != i 各行的第 i 列。
        let pivot_row: [f32; 4] = a[i];
        let pivot_b = b[i];
        for (r, row) in a.iter_mut().enumerate() {
            if r == i {
                continue;
            }
            let factor = row[i];
            for (cell, pivot_cell) in row.iter_mut().zip(pivot_row.iter()).skip(i) {
                *cell -= factor * pivot_cell;
            }
            b[r] -= factor * pivot_b;
        }
    }
    Ok(b)
}

/// 反向映射：对每个目标 `(u, v)` 解出源 `(sx, sy)` 后双线性采样填充。
fn warp_inverse(image: &RgbImage, sim: &Similarity, side: u32) -> RgbImage {
    let mut out = RgbImage::new(side, side);
    let det = sim.a.mul_add(sim.a, sim.b * sim.b);
    if det < f32::EPSILON || image.width() == 0 || image.height() == 0 {
        return out;
    }
    let inv_det = 1.0 / det;
    for v in 0..side {
        for u in 0..side {
            #[expect(
                clippy::cast_precision_loss,
                reason = "side <= 112 远小于 f32 mantissa 24 bit 精度边界"
            )]
            let (uf, vf) = (u as f32, v as f32);
            let du = uf - sim.tx;
            let dv = vf - sim.ty;
            let sx = sim.a.mul_add(du, sim.b * dv) * inv_det;
            let sy = (-sim.b).mul_add(du, sim.a * dv) * inv_det;
            out.put_pixel(u, v, bilinear_sample(image, sx, sy));
        }
    }
    out
}

/// 双线性采样。源坐标超出 `[0, w-1] × [0, h-1]` 全黑兜底（OpenCV `BORDER_CONSTANT(0)` 套路）。
#[expect(
    clippy::many_single_char_names,
    reason = "x/y/w/h 是图像坐标与尺寸标准缩写；改长名反损可读性"
)]
fn bilinear_sample(image: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let w = image.width();
    let h = image.height();
    let x_floor = x.floor();
    let y_floor = y.floor();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "floor 后越界由 sample_clamped 内 4 邻域 in_bounds 兜底（已 clamp 到 0）"
    )]
    let (x0, y0) = (x_floor as i64, y_floor as i64);
    let (x1, y1) = (x0 + 1, y0 + 1);
    let fx = x - x_floor;
    let fy = y - y_floor;
    let p00 = sample_clamped(image, x0, y0, w, h);
    let p01 = sample_clamped(image, x1, y0, w, h);
    let p10 = sample_clamped(image, x0, y1, w, h);
    let p11 = sample_clamped(image, x1, y1, w, h);
    let weights = [
        (1.0 - fx) * (1.0 - fy),
        fx * (1.0 - fy),
        (1.0 - fx) * fy,
        fx * fy,
    ];
    let mut rgb = [0_u8; 3];
    for ch in 0..3 {
        let v = p00[ch].mul_add(
            weights[0],
            p01[ch].mul_add(
                weights[1],
                p10[ch].mul_add(weights[2], p11[ch] * weights[3]),
            ),
        );
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamp [0,255] 后 round 必在 u8 范围"
        )]
        let byte = v.round().clamp(0.0, 255.0) as u8;
        rgb[ch] = byte;
    }
    Rgb(rgb)
}

/// 越界返 `[0, 0, 0]`，让外层加权和按零像素处理。
fn sample_clamped(image: &RgbImage, x: i64, y: i64, w: u32, h: u32) -> [f32; 3] {
    if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
        return [0.0; 3];
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "上一行已守 0 <= x < w（u32 范围内）"
    )]
    let (xu, yu) = (x as u32, y as u32);
    let px = image.get_pixel(xu, yu);
    [f32::from(px.0[0]), f32::from(px.0[1]), f32::from(px.0[2])]
}

#[cfg(test)]
#[path = "face_align_tests.rs"]
mod tests;
