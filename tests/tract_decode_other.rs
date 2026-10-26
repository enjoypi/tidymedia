#![allow(clippy::manual_is_multiple_of)]

use tract_onnx::prelude::*;

use tidymedia::{dbnet_preprocess, decide};

fn tensor_f32(shape: &[usize], data: &[f32]) -> Tensor {
    tract_ndarray::Array::from_shape_vec(shape, data.to_vec())
        .unwrap()
        .into_tensor()
}

#[test]
fn dbnet_decide_handles_empty_low_high() {
    let empty = tensor_f32(&[1, 1, 0], &[]);
    assert!(!decide(&empty, 0.3, 0.005).unwrap());
    let low = tensor_f32(&[1, 1, 4], &[0.1, 0.2, 0.1, 0.2]);
    assert!(!decide(&low, 0.3, 0.005).unwrap());
    let hit = tensor_f32(&[1, 1, 4], &[0.9, 0.9, 0.1, 0.2]);
    assert!(decide(&hit, 0.3, 0.01).unwrap());
}

#[test]
fn dbnet_preprocess_resizes_beyond_max_side() {
    let bytes = image_png(400, 300);
    let t = dbnet_preprocess(&bytes, 64).unwrap();
    assert!(t.shape()[2] % 32 == 0 && t.shape()[3] % 32 == 0);
    assert!(t.shape()[2] >= 32 && t.shape()[3] >= 32);
}

#[test]
fn dbnet_preprocess_keeps_small_image_shape() {
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

#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;
