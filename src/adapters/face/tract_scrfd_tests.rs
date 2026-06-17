//! `tract_scrfd` 单元测试：装配 + preprocess（letterbox + `ScaleMeta`）+ stub 注入。
//! anchor 解码 + NMS 算法在 `_real.rs` 由 ignore-regex 排除（e2e 真跑步骤 6 验证）。

use super::*;
use camino::Utf8Path;

struct ConstRaw {
    detections: Vec<FaceDetection>,
}

impl RawScrfd for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Vec<FaceDetection>> {
        Ok(self.detections.clone())
    }
}

struct FailRaw;
impl RawScrfd for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Vec<FaceDetection>> {
        Err(io::Error::other("stub scrfd failed"))
    }
}

fn cfg() -> FaceConfig {
    FaceConfig {
        scrfd_model_path: "ignored-by-stub".into(),
        ..FaceConfig::default()
    }
}

fn tiny_png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&[255_u8, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
        .expect("encode tiny png");
    out
}

fn sample_face() -> FaceDetection {
    FaceDetection {
        bbox: [10.0, 20.0, 100.0, 200.0],
        score: 0.95,
        landmarks_5pt: [[1.0; 2]; 5],
    }
}

#[test]
fn build_scrfd_detector_rejects_empty_path() {
    let e = build_scrfd_detector(&FaceConfig::default()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("scrfd_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_scrfd_detector_rejects_whitespace_only_path() {
    let c = FaceConfig {
        scrfd_model_path: "   ".into(),
        ..FaceConfig::default()
    };
    let e = build_scrfd_detector(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn detect_faces_returns_stub_detections() {
    let det = TractScrfdDetector::with_raw(
        cfg(),
        Box::new(ConstRaw {
            detections: vec![sample_face()],
        }),
    );
    let faces = det
        .detect_faces(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap();
    assert_eq!(faces.len(), 1);
    assert!((faces[0].score - 0.95).abs() < f32::EPSILON);
}

#[test]
fn detect_faces_returns_empty_when_no_detections() {
    let det = TractScrfdDetector::with_raw(cfg(), Box::new(ConstRaw { detections: vec![] }));
    let faces = det
        .detect_faces(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap();
    assert!(faces.is_empty());
}

#[test]
fn detect_faces_propagates_raw_error() {
    let det = TractScrfdDetector::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .detect_faces(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap_err();
    assert!(e.to_string().contains("stub scrfd failed"), "got: {e}");
}

#[test]
fn detect_faces_returns_invalid_data_on_bad_bytes() {
    let det = TractScrfdDetector::with_raw(cfg(), Box::new(ConstRaw { detections: vec![] }));
    let e = det
        .detect_faces(Utf8Path::new("/bad"), b"not-an-image")
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn preprocess_returns_640x640_tensor() {
    let (t, _meta) = preprocess(&tiny_png()).unwrap();
    assert_eq!(t.shape(), [1, 3, 640, 640]);
}

#[test]
fn preprocess_meta_records_scale_and_padding() {
    // 1×1 → scale = 640，padding = (640-1)/2 = 319（letterbox 居中）
    let (_t, meta) = preprocess(&tiny_png()).unwrap();
    assert!((meta.scale - 640.0).abs() < 1.0);
    // pad_x/pad_y 应非负（图像缩到 ≤ 640 后居中）
    assert!(meta.pad_x >= 0.0);
    assert!(meta.pad_y >= 0.0);
}

#[test]
fn preprocess_landscape_image_padded_vertically() {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let pixels = vec![128_u8; 640 * 320 * 3];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 640, 320, image::ExtendedColorType::Rgb8)
        .unwrap();
    let (_t, meta) = preprocess(&out).unwrap();
    // scale = 640/640 = 1.0；new_h = 320；pad_y = (640-320)/2 = 160
    assert!((meta.scale - 1.0).abs() < 0.01);
    assert!((meta.pad_y - 160.0).abs() < 1.0);
    assert!(meta.pad_x.abs() < 1.0);
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractScrfdDetector::with_raw(cfg(), Box::new(ConstRaw { detections: vec![] }));
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}
