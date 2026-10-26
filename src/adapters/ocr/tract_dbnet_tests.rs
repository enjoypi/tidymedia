use super::*;
use camino::Utf8Path;

fn tiny_png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&[255_u8, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
        .expect("encode tiny png");
    out
}

fn solid_rgb_png(width: u32, height: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let pixels = vec![30_u8; width as usize * height as usize * 3];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgb8)
        .expect("encode solid png");
    out
}

fn tensor_f32(shape: &[usize], data: &[f32]) -> Tensor {
    tract_ndarray::Array::from_shape_vec(shape, data.to_vec())
        .expect("stub tensor shape")
        .into_tensor()
}

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
        max_image_bytes: 50 * 1024 * 1024,
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
        "ratio=1.0 > 0.005"
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
        "ratio=0"
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
    let t = preprocess(&tiny_png(), 736).unwrap();
    let shape = t.shape();
    assert_eq!(shape, [1, 3, 32, 32], "got: {shape:?}");
}

#[test]
fn preprocess_caps_max_side_then_aligns() {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let pixels = vec![128_u8; 800 * 600 * 3];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 800, 600, image::ExtendedColorType::Rgb8)
        .unwrap();
    let t = preprocess(&out, 736).unwrap();
    let shape = t.shape();
    assert_eq!(shape, [1, 3, 576, 736], "got: {shape:?}");
}

#[test]
fn preprocess_resizes_beyond_max_side_then_aligns() {
    let t = preprocess(&solid_rgb_png(400, 300), 64).unwrap();
    let shape = t.shape();
    assert_eq!(shape, [1, 3, 64, 64], "got: {shape:?}");
}

#[test]
fn preprocess_keeps_small_image_at_original_size() {
    let t = preprocess(&solid_rgb_png(64, 64), 128).unwrap();
    let shape = t.shape();
    assert_eq!(shape, [1, 3, 64, 64], "got: {shape:?}");
}

#[test]
fn preprocess_rejects_zero_byte_input() {
    let e = preprocess(b"", 736).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn decide_returns_false_on_empty_tensor() {
    let t = tract_ndarray::Array4::<f32>::zeros((1, 1, 0, 0)).into_tensor();
    assert!(!decide(&t, 0.3, 0.005));
}

#[test]
fn decide_handles_empty_low_and_high_outputs() {
    let empty = tensor_f32(&[1, 1, 0], &[]);
    assert!(!decide(&empty, 0.3, 0.005));
    let low = tensor_f32(&[1, 1, 4], &[0.1, 0.2, 0.1, 0.2]);
    assert!(!decide(&low, 0.3, 0.005));
    let high = tensor_f32(&[1, 1, 4], &[0.9, 0.9, 0.1, 0.2]);
    assert!(decide(&high, 0.3, 0.01));
}

#[test]
fn decide_boundary_ratio_strictly_greater() {
    let mut data = vec![0.0_f32; 100];
    data[0] = 0.5;
    let t = tract_ndarray::Array4::from_shape_vec((1, 1, 10, 10), data)
        .unwrap()
        .into_tensor();
    assert!(decide(&t, 0.3, 0.005));

    let mut data = vec![0.0_f32; 1000];
    data[0] = 0.5;
    let t = tract_ndarray::Array4::from_shape_vec((1, 1, 25, 40), data)
        .unwrap()
        .into_tensor();
    assert!(!decide(&t, 0.3, 0.005));
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

static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[expect(clippy::unnecessary_wraps, reason = "与生产 loader 签名一致")]
fn load_counting(_: &std::path::Path) -> io::Result<Box<dyn RawDetector>> {
    LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(Box::new(ConstRaw {
        value: 0.9,
        h: 32,
        w: 32,
    }))
}

#[test]
fn ensure_raw_loads_once_and_reuses_detector() {
    LOADS.store(0, std::sync::atomic::Ordering::SeqCst);
    let det = TractDbnetDetector {
        cfg: cfg(),
        raw: Mutex::new(None),
    };
    det.ensure_raw_with(load_counting).unwrap();
    det.ensure_raw_with(load_counting).unwrap();
    assert!(det.raw.lock().is_some());
    assert_eq!(LOADS.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn has_text_propagates_model_load_error() {
    let det = build_detector(&cfg()).unwrap();
    let err = det
        .has_text(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap_err();
    assert!(err.to_string().contains("load DBNet ONNX"), "got: {err}");
}

#[test]
fn target_size_keeps_small_image_at_min_32() {
    let (w, h) = target_size(1, 1, 736);
    assert_eq!((w, h), (32, 32));
}

#[test]
fn target_size_scales_down_oversized_image() {
    let (w, h) = target_size(2000, 1000, 736);
    assert_eq!((w, h), (736, 384));
}
