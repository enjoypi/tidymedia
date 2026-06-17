//! `tract_eyestate` 单元测试。

use super::*;
use camino::Utf8Path;

struct ConstRaw {
    logits: [f32; 2],
}

impl RawEyeState for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        let t = tract_ndarray::Array2::from_shape_vec((1, 2), self.logits.to_vec())
            .expect("stub shape")
            .into_tensor();
        Ok(t)
    }
}

struct FailRaw;
impl RawEyeState for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Err(io::Error::other("stub eyestate failed"))
    }
}

fn cfg() -> FaceConfig {
    FaceConfig {
        eyestate_model_path: "ignored-by-stub".into(),
        ..FaceConfig::default()
    }
}

fn tiny_eye() -> image::RgbImage {
    image::RgbImage::from_pixel(8, 8, image::Rgb([50, 50, 50]))
}

#[test]
fn build_eyestate_classifier_rejects_empty_path() {
    let e = build_eyestate_classifier(&FaceConfig::default()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("eyestate_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_eyestate_classifier_rejects_whitespace_only_path() {
    let c = FaceConfig {
        eyestate_model_path: "   ".into(),
        ..FaceConfig::default()
    };
    let e = build_eyestate_classifier(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn classify_eye_high_when_closed_logit_dominates() {
    // logits [open=0.0, closed=5.0] → softmax 接近 1.0
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(ConstRaw { logits: [0.0, 5.0] }));
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!(p > 0.99, "got: {p}");
}

#[test]
fn classify_eye_low_when_open_logit_dominates() {
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(ConstRaw { logits: [5.0, 0.0] }));
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!(p < 0.01, "got: {p}");
}

#[test]
fn classify_eye_balanced_at_half() {
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(ConstRaw { logits: [1.0, 1.0] }));
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!((p - 0.5).abs() < 1e-5, "got: {p}");
}

#[test]
fn classify_eye_propagates_raw_error() {
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .classify_eye(Utf8Path::new("/x"), &tiny_eye())
        .unwrap_err();
    assert!(e.to_string().contains("stub eyestate failed"), "got: {e}");
}

#[test]
fn decode_rejects_short_output() {
    let t = tract_ndarray::Array1::from_vec(vec![0.0_f32]).into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(e.to_string().contains("len 1 < expected 2"), "got: {e}");
}

#[test]
fn preprocess_resizes_non_64() {
    let big = image::RgbImage::from_pixel(128, 64, image::Rgb([10, 20, 30]));
    let t = preprocess(&big);
    assert_eq!(t.shape(), [1, 3, 64, 64]);
}

#[test]
fn preprocess_keeps_64_unchanged_shape() {
    let exact = image::RgbImage::from_pixel(64, 64, image::Rgb([10, 20, 30]));
    let t = preprocess(&exact);
    assert_eq!(t.shape(), [1, 3, 64, 64]);
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(ConstRaw { logits: [0.0, 0.0] }));
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}
