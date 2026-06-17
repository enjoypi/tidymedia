//! `tract_mobilefacenet` 单元测试。

use super::*;
use camino::Utf8Path;

struct ConstRaw {
    value: f32,
    dim: usize,
}

impl RawFacenet for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        let v = vec![self.value; self.dim];
        let t = tract_ndarray::Array2::from_shape_vec((1, self.dim), v)
            .expect("stub facenet shape")
            .into_tensor();
        Ok(t)
    }
}

struct FailRaw;
impl RawFacenet for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Err(io::Error::other("stub facenet failed"))
    }
}

fn cfg() -> FaceConfig {
    FaceConfig {
        facenet_model_path: "ignored-by-stub".into(),
        ..FaceConfig::default()
    }
}

fn tiny_face() -> image::RgbImage {
    image::RgbImage::from_pixel(4, 4, image::Rgb([128, 64, 200]))
}

#[test]
fn build_facenet_embedder_rejects_empty_path() {
    let c = FaceConfig::default();
    let e = build_facenet_embedder(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("facenet_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_facenet_embedder_rejects_whitespace_only_path() {
    let c = FaceConfig {
        facenet_model_path: "   ".into(),
        ..FaceConfig::default()
    };
    let e = build_facenet_embedder(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn embed_face_returns_l2_normalized_128() {
    let det = TractFacenetEmbedder::with_raw(
        cfg(),
        Box::new(ConstRaw {
            value: 1.0,
            dim: 128,
        }),
    );
    let emb = det
        .embed_face(Utf8Path::new("/x.jpg"), &tiny_face())
        .unwrap();
    // L2: all-ones vector of len 128 → 1/sqrt(128) per element
    let expected = 1.0_f32 / (128.0_f32).sqrt();
    for v in &emb {
        assert!((v - expected).abs() < 1e-5, "got: {v}");
    }
    // norm = 1
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
}

#[test]
fn embed_face_zero_vector_remains_zero() {
    let det = TractFacenetEmbedder::with_raw(
        cfg(),
        Box::new(ConstRaw {
            value: 0.0,
            dim: 128,
        }),
    );
    let emb = det
        .embed_face(Utf8Path::new("/x.jpg"), &tiny_face())
        .unwrap();
    assert!(emb.iter().all(|v| v.abs() < f32::EPSILON));
}

#[test]
fn embed_face_propagates_raw_error() {
    let det = TractFacenetEmbedder::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .embed_face(Utf8Path::new("/x.jpg"), &tiny_face())
        .unwrap_err();
    assert!(e.to_string().contains("stub facenet failed"), "got: {e}");
}

#[test]
fn decode_rejects_short_output() {
    let t = tract_ndarray::Array2::from_shape_vec((1, 64), vec![0.0_f32; 64])
        .unwrap()
        .into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(e.to_string().contains("dim 64 < expected 128"), "got: {e}");
}

#[test]
fn preprocess_accepts_non_112_by_resizing() {
    let big = image::RgbImage::from_pixel(256, 256, image::Rgb([10, 20, 30]));
    let t = preprocess(&big);
    assert_eq!(t.shape(), [1, 3, 112, 112]);
}

#[test]
fn preprocess_keeps_112_unchanged_shape() {
    let exact = image::RgbImage::from_pixel(112, 112, image::Rgb([10, 20, 30]));
    let t = preprocess(&exact);
    assert_eq!(t.shape(), [1, 3, 112, 112]);
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractFacenetEmbedder::with_raw(
        cfg(),
        Box::new(ConstRaw {
            value: 0.0,
            dim: 128,
        }),
    );
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}
