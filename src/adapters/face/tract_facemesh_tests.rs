//! `tract_facemesh` 单元测试。

use super::*;
use camino::Utf8Path;

struct ConstRaw {
    value: f32,
}

impl RawFaceMesh for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        let v = vec![self.value; 468 * 3];
        let t = tract_ndarray::Array2::from_shape_vec((1, 1404), v)
            .expect("stub shape")
            .into_tensor();
        Ok(t)
    }
}

struct FailRaw;
impl RawFaceMesh for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Err(io::Error::other("stub facemesh failed"))
    }
}

fn cfg() -> FaceConfig {
    FaceConfig {
        facemesh_model_path: "ignored-by-stub".into(),
        ..FaceConfig::default()
    }
}

fn tiny_crop() -> image::RgbImage {
    image::RgbImage::from_pixel(4, 4, image::Rgb([128, 64, 200]))
}

#[test]
fn build_facemesh_rejects_empty_path() {
    let e = build_facemesh(&FaceConfig::default()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("facemesh_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_facemesh_rejects_whitespace_only_path() {
    let c = FaceConfig {
        facemesh_model_path: "   ".into(),
        ..FaceConfig::default()
    };
    let e = build_facemesh(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn detect_mesh_returns_468_points() {
    let det = TractFaceMeshDetector::with_raw(cfg(), Box::new(ConstRaw { value: 0.5 }));
    let m = det.detect_mesh(Utf8Path::new("/x"), &tiny_crop()).unwrap();
    assert_eq!(m.len(), 468);
    for p in &m {
        assert!((p[0] - 0.5).abs() < f32::EPSILON);
        assert!((p[1] - 0.5).abs() < f32::EPSILON);
        assert!((p[2] - 0.5).abs() < f32::EPSILON);
    }
}

#[test]
fn detect_mesh_propagates_raw_error() {
    let det = TractFaceMeshDetector::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .detect_mesh(Utf8Path::new("/x"), &tiny_crop())
        .unwrap_err();
    assert!(e.to_string().contains("stub facemesh failed"), "got: {e}");
}

#[test]
fn decode_rejects_short_output() {
    let t = tract_ndarray::Array1::from_vec(vec![0.0_f32; 100]).into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(
        e.to_string().contains("len 100 < expected 1404"),
        "got: {e}"
    );
}

#[test]
fn preprocess_resizes_non_192() {
    let big = image::RgbImage::from_pixel(256, 256, image::Rgb([10, 20, 30]));
    let t = preprocess(&big).unwrap();
    assert_eq!(t.shape(), [1, 3, 192, 192]);
}

#[test]
fn preprocess_keeps_192_unchanged_shape() {
    let exact = image::RgbImage::from_pixel(192, 192, image::Rgb([10, 20, 30]));
    let t = preprocess(&exact).unwrap();
    assert_eq!(t.shape(), [1, 3, 192, 192]);
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractFaceMeshDetector::with_raw(cfg(), Box::new(ConstRaw { value: 0.0 }));
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}
