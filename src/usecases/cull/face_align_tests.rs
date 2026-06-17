//! `face_align` 单测：恒等映射、缩放映射、NaN 拒绝、奇异矩阵拒绝、越界采样兜底。

use super::*;

const TEMPLATE_POINTS: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

fn gradient_image(side: u32) -> RgbImage {
    let mut img = RgbImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let r = u8::try_from(x % 255).expect("internal: x % 255 < 256");
            let g = u8::try_from(y % 255).expect("internal: y % 255 < 256");
            img.put_pixel(x, y, Rgb([r, g, 128]));
        }
    }
    img
}

#[test]
fn align_face_rejects_nan_landmarks() {
    let img = RgbImage::new(112, 112);
    let mut lm = TEMPLATE_POINTS;
    lm[0][0] = f32::NAN;
    let err = align_face(&img, &lm).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("finite"), "got: {err}");
}

#[test]
fn align_face_rejects_infinite_landmarks() {
    let img = RgbImage::new(112, 112);
    let mut lm = TEMPLATE_POINTS;
    lm[2][1] = f32::INFINITY;
    let err = align_face(&img, &lm).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn align_face_identity_landmarks_returns_template_region() {
    // landmarks 即模板 → 相似变换为恒等 → 输出 = 源图前 112×112 区域。
    let img = gradient_image(112);
    let aligned = align_face(&img, &TEMPLATE_POINTS).unwrap();
    assert_eq!(aligned.width(), 112);
    assert_eq!(aligned.height(), 112);
    // 采几个采样点对比（双线性精度 ±1）
    let center_src = img.get_pixel(56, 56);
    let center_dst = aligned.get_pixel(56, 56);
    for ch in 0..3 {
        let diff = i16::from(center_src.0[ch]) - i16::from(center_dst.0[ch]);
        assert!(
            diff.abs() <= 1,
            "ch{ch} src={center_src:?} dst={center_dst:?}"
        );
    }
}

#[test]
fn align_face_scaled_landmarks_samples_doubled_source() {
    // landmarks = TEMPLATE × 2 → 相似变换 sx=0.5 → dst (u,v) 采源 (2u, 2v)。
    let img = gradient_image(224);
    let mut lm = TEMPLATE_POINTS;
    for pt in &mut lm {
        pt[0] *= 2.0;
        pt[1] *= 2.0;
    }
    let aligned = align_face(&img, &lm).unwrap();
    // dst (50, 50) → src (100, 100)
    let expected = img.get_pixel(100, 100);
    let actual = aligned.get_pixel(50, 50);
    for ch in 0..3 {
        let diff = i16::from(expected.0[ch]) - i16::from(actual.0[ch]);
        assert!(
            diff.abs() <= 1,
            "ch{ch} expected={expected:?} actual={actual:?}"
        );
    }
}

#[test]
fn align_face_rejects_singular_when_all_points_collinear() {
    // 5 点全相同 → ATA 秩 < 4 → solve_4x4 任一列主元 < EPS → InvalidInput。
    let img = RgbImage::new(112, 112);
    let lm = [[1.0_f32, 1.0]; 5];
    let err = align_face(&img, &lm).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("singular"), "got: {err}");
}

#[test]
fn align_face_empty_source_returns_black_output() {
    // 0×0 源图 → warp_inverse 跳过采样直接返 112×112 全黑。
    let img = RgbImage::new(0, 0);
    let aligned = align_face(&img, &TEMPLATE_POINTS).unwrap();
    assert_eq!(aligned.width(), 112);
    let p = aligned.get_pixel(0, 0);
    assert_eq!(p.0, [0, 0, 0]);
}

#[test]
fn warp_inverse_zero_determinant_returns_black() {
    // a=b=0 → det=0 → warp_inverse 早返全黑（不可达：fit_similarity 不会产 0 det，
    // 此处直测内部边界）。
    let img = gradient_image(16);
    let sim = Similarity {
        a: 0.0,
        b: 0.0,
        tx: 0.0,
        ty: 0.0,
    };
    let out = warp_inverse(&img, &sim, 8);
    assert!(out.pixels().all(|p| p.0 == [0, 0, 0]));
}

#[test]
fn bilinear_sample_out_of_bounds_returns_black() {
    // 源 16×16，采样 (-1, -1) / (100, 100) 都越界 → 4 邻域全部 sample_clamped 返 0
    // → 加权和 0 → 输出 [0,0,0]。
    let img = gradient_image(16);
    let p = bilinear_sample(&img, -1.0, -1.0);
    assert_eq!(p.0, [0, 0, 0]);
    let q = bilinear_sample(&img, 100.0, 100.0);
    assert_eq!(q.0, [0, 0, 0]);
}

#[test]
fn solve_4x4_recovers_identity() {
    // [1 0 0 0; 0 1 0 0; 0 0 1 0; 0 0 0 1] x = [a b c d]  → x = [a b c d]
    let a = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let b = [1.0, 2.0, 3.0, 4.0];
    let x = solve_4x4(a, b).unwrap();
    for (got, want) in x.iter().zip([1.0_f32, 2.0, 3.0, 4.0]) {
        assert!((got - want).abs() < 1e-5, "x={x:?}");
    }
}

#[test]
fn solve_4x4_handles_partial_pivoting() {
    // 第一列首行为 0 → 必须 swap 才能消元；否则除以 0 触发 SINGULAR_EPS Err。
    let a = [
        [0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let b = [5.0, 7.0, 11.0, 13.0];
    let x = solve_4x4(a, b).unwrap();
    // 行 swap 后等价于 [a b c d] = [7 5 11 13]（行 0/1 交换后 x[0]=b_swapped[0]=7）
    assert!((x[0] - 7.0).abs() < 1e-5, "got: {x:?}");
    assert!((x[1] - 5.0).abs() < 1e-5);
}

#[test]
fn solve_4x4_rejects_singular_matrix() {
    let a = [[0.0_f32; 4]; 4];
    let b = [1.0, 2.0, 3.0, 4.0];
    let err = solve_4x4(a, b).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("singular"));
}

#[test]
fn fit_similarity_recovers_pure_translation() {
    // landmarks = TEMPLATE 平移 (+5, -3) → 期望 (a, b, tx, ty) ≈ (1, 0, -5, +3)。
    // 注意：src→dst 是 dst = T(src)，TEMPLATE 是 dst，平移后的是 src；解出来的
    // T 把 (TEMPLATE+δ) 映回 TEMPLATE，所以 tx = -δx, ty = -δy。
    let mut lm = TEMPLATE_POINTS;
    for pt in &mut lm {
        pt[0] += 5.0;
        pt[1] -= 3.0;
    }
    let sim = fit_similarity(&lm).unwrap();
    assert!((sim.a - 1.0).abs() < 1e-3, "a={}", sim.a);
    assert!(sim.b.abs() < 1e-3, "b={}", sim.b);
    assert!((sim.tx + 5.0).abs() < 1e-2, "tx={}", sim.tx);
    assert!((sim.ty - 3.0).abs() < 1e-2, "ty={}", sim.ty);
}
