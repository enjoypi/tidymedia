//! `tract_dbnet` 单元测试：装配 + 前/后处理 + Raw 注入 stub 测三态。
//!
//! 真实 tract `model.run` 走 `_real.rs` 由 ignore-regex 排除；本文件用 `RawDetector`
//! trait 注入 stub model 让前/后处理 + `has_text` 主路径 100% 覆盖。

use super::*;
use camino::Utf8Path;

/// 构造极小 PNG（1×1 红像素）供 preprocess 用——image crate 必能解码。
fn tiny_png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&[255_u8, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
        .expect("encode tiny png");
    out
}

/// Stub `RawDetector`：返指定值的常量 sigmoid map，让 decide 阈值逻辑可测。
struct ConstRaw {
    value: f32,
    h: usize,
    w: usize,
}

impl RawDetector for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        let v = vec![self.value; self.h * self.w];
        let t = tract_ndarray::Array4::from_shape_vec((1, 1, self.h, self.w), v)
            .expect("stub raw shape")
            .into_tensor();
        Ok(t)
    }
}

struct FailRaw;
impl RawDetector for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Err(io::Error::other("stub raw failed"))
    }
}

fn cfg() -> OcrConfig {
    OcrConfig {
        det_model_path: "ignored-by-stub".into(),
        binarize_threshold: 0.3,
        min_text_pixel_ratio: 0.005,
        resize_max_side: 736,
    }
}

#[test]
fn build_detector_rejects_empty_model_path() {
    let mut c = cfg();
    c.det_model_path = String::new();
    let e = build_detector(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("det_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_detector_rejects_whitespace_only_path() {
    let mut c = cfg();
    c.det_model_path = "   ".into();
    let e = build_detector(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn has_text_true_when_sigmoid_above_threshold() {
    let raw = Box::new(ConstRaw {
        value: 0.9,
        h: 32,
        w: 32,
    });
    let det = TractDbnetDetector::with_raw(cfg(), raw);
    assert!(
        det.has_text(Utf8Path::new("/x.png"), &tiny_png()).unwrap(),
        "0.9 > 0.3 in all 1024 pixels → ratio=1.0 > 0.005 → true"
    );
}

#[test]
fn has_text_false_when_sigmoid_below_threshold() {
    let raw = Box::new(ConstRaw {
        value: 0.1,
        h: 32,
        w: 32,
    });
    let det = TractDbnetDetector::with_raw(cfg(), raw);
    assert!(
        !det.has_text(Utf8Path::new("/x.png"), &tiny_png()).unwrap(),
        "0.1 < 0.3 → no hit → ratio=0 → false"
    );
}

#[test]
fn has_text_propagates_raw_error() {
    let det = TractDbnetDetector::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .has_text(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap_err();
    assert!(e.to_string().contains("stub raw failed"), "got: {e}");
}

#[test]
fn has_text_returns_invalid_data_on_bad_image_bytes() {
    let det = TractDbnetDetector::with_raw(
        cfg(),
        Box::new(ConstRaw {
            value: 0.9,
            h: 32,
            w: 32,
        }),
    );
    let e = det
        .has_text(Utf8Path::new("/bad.png"), b"not-an-image")
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    assert!(e.to_string().contains("decode image"), "got: {e}");
}

#[test]
fn preprocess_aligns_to_32_min_size() {
    // 1×1 → 32×32（最小对齐边界）
    let t = preprocess(&tiny_png(), 736).unwrap();
    let shape = t.shape();
    assert_eq!(shape, [1, 3, 32, 32], "got: {shape:?}");
}

#[test]
fn preprocess_caps_max_side_then_aligns() {
    // 构造 800×600 PNG → max_side=736 → 长边按比例缩 → 32 对齐
    use image::ImageEncoder;
    let mut out = Vec::new();
    let pixels = vec![128_u8; 800 * 600 * 3];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 800, 600, image::ExtendedColorType::Rgb8)
        .unwrap();
    let t = preprocess(&out, 736).unwrap();
    let shape = t.shape();
    // 800*736/800=736 ; 600*736/800=552 → 32 对齐：736, 576
    assert_eq!(shape, [1, 3, 576, 736], "got: {shape:?}");
}

#[test]
fn preprocess_rejects_zero_byte_input() {
    // image crate 解 0 字节失败 → preprocess InvalidData
    let e = preprocess(b"", 736).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn decide_returns_false_on_empty_tensor() {
    let t = tract_ndarray::Array4::<f32>::zeros((1, 1, 0, 0)).into_tensor();
    assert!(!decide(&t, 0.3, 0.005).unwrap());
}

#[test]
fn decide_boundary_ratio_strictly_greater() {
    // 100 像素中刚好 1 个超阈值 → ratio=0.01 > 0.005 → true
    let mut data = vec![0.0_f32; 100];
    data[0] = 0.5;
    let t = tract_ndarray::Array4::from_shape_vec((1, 1, 10, 10), data)
        .unwrap()
        .into_tensor();
    assert!(decide(&t, 0.3, 0.005).unwrap());

    // 1000 像素中 1 个超阈值 → ratio=0.001 < 0.005 → false
    let mut data = vec![0.0_f32; 1000];
    data[0] = 0.5;
    let t = tract_ndarray::Array4::from_shape_vec((1, 1, 25, 40), data)
        .unwrap()
        .into_tensor();
    assert!(!decide(&t, 0.3, 0.005).unwrap());
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractDbnetDetector::with_raw(
        cfg(),
        Box::new(ConstRaw {
            value: 0.1,
            h: 32,
            w: 32,
        }),
    );
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}

#[test]
fn align32_handles_boundaries() {
    assert_eq!(align32(0), 0);
    assert_eq!(align32(1), 32);
    assert_eq!(align32(32), 32);
    assert_eq!(align32(33), 64);
}

#[test]
fn target_size_keeps_small_image_at_min_32() {
    let (w, h) = target_size(1, 1, 736);
    assert_eq!((w, h), (32, 32));
}

#[test]
fn target_size_scales_down_oversized_image() {
    let (w, h) = target_size(2000, 1000, 736);
    // 长边 2000 → 736 ；scale=0.368 ；2000*0.368=736, 1000*0.368=368 → 对齐 736, 384
    assert_eq!((w, h), (736, 384));
}
