#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::manual_is_multiple_of,
    clippy::redundant_clone,
    clippy::single_char_pattern,
    clippy::unnecessary_trailing_comma
)]

//! eyestate `decode`/`preprocess` 与 dbnet `decide`/`preprocess` 的边界覆盖。
//! 真模型输出恒为合法 shape/非空 slice，错误分支与零像素分支只能直调。

use tract_onnx::prelude::*;

use tidymedia::{dbnet_preprocess, decide, eyestate_decode, eyestate_preprocess};

fn tensor_f32(shape: &[usize], data: &[f32]) -> Tensor {
    tract_ndarray::Array::from_shape_vec(shape, data.to_vec())
        .unwrap()
        .into_tensor()
}

#[test]
fn eyestate_decode_rejects_wrong_rank_and_zero_anchors() {
    // shape.len() != 3。
    let bad = tensor_f32(&[1, 6], &[0.0; 6]);
    let e = eyestate_decode(&bad).unwrap_err();
    assert!(e.to_string().contains("shape"), "got: {e}");
    // anchors == 0。
    let zero = tensor_f32(&[1, 6, 0], &[]);
    let e2 = eyestate_decode(&zero).unwrap_err();
    assert!(e2.to_string().contains("0 anchors"), "got: {e2}");
}

#[test]
fn eyestate_decode_clamps_closed_conf() {
    // closed 通道（index 5）在各 anchor 取 max conf。
    let mut data = vec![0.0_f32; 6 * 4];
    for a in 0..4 {
        data[5 * 4 + a] = 1.3 - a as f32 * 0.1; // 1.3 .. 1.0
    }
    let t = tensor_f32(&[1, 6, 4], &data);
    let p = eyestate_decode(&t).unwrap();
    assert_eq!(p, 1.0, "clamp 到 1.0");
    let mut low = vec![0.0_f32; 6 * 3];
    low[5 * 3 + 1] = -0.2;
    let t2 = tensor_f32(&[1, 6, 3], &low);
    let p2 = eyestate_decode(&t2).unwrap();
    assert_eq!(p2, 0.0, "clamp 到 0.0");
}

#[test]
fn eyestate_preprocess_zero_sized_canvas() {
    // 0×0 输入：跳过 letterbox overlay，返回全 114 padding 的 NCHW 画布。
    let t = eyestate_preprocess(&image::RgbImage::new(0, 0)).unwrap();
    assert_eq!(t.shape(), &[1, 3, 640, 640]);
}

#[test]
fn dbnet_decide_handles_empty_low_high() {
    // 空 slice → false（early return）。
    let empty = tensor_f32(&[1, 1, 0], &[]);
    assert!(!decide(&empty, 0.3, 0.005).unwrap());
    // 全部低于阈值 → false。
    let low = tensor_f32(&[1, 1, 4], &[0.1, 0.2, 0.1, 0.2]);
    assert!(!decide(&low, 0.3, 0.005).unwrap());
    // 高于阈值占比超 `min_text_pixel_ratio` → true。
    let hit = tensor_f32(&[1, 1, 4], &[0.9, 0.9, 0.1, 0.2]);
    assert!(decide(&hit, 0.3, 0.01).unwrap());
}

#[test]
fn dbnet_preprocess_resizes_beyond_max_side() {
    // 大图 + 小 max_side → `target_size` 走 scale < 1 分支，宽高按 32 对齐。
    let bytes = image_png(400, 300);
    let t = dbnet_preprocess(&bytes, 64).unwrap();
    assert!(t.shape()[2] % 32 == 0 && t.shape()[3] % 32 == 0);
    assert!(t.shape()[2] >= 32 && t.shape()[3] >= 32);
}

#[test]
fn dbnet_preprocess_keeps_small_image_shape() {
    // 小图 max_side 大 → scale=1，仍 32 对齐。
    let bytes = image_png(64, 64);
    let t = dbnet_preprocess(&bytes, 128).unwrap();
    assert_eq!(t.shape()[3], 64);
}

fn image_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for p in img.pixels_mut() {
        *p = image::Rgb([30, 60, 90]);
    }
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("in-memory PNG encode");
    buf
}
