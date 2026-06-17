//! 测试用 4 个 face Gateway 实现：路径查表 + Err 注入。与 `FakeTextDetector` 同套路。

use std::collections::{HashMap, HashSet};
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::Mutex;

use super::{EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector};

// ───────────────────── FakeFaceDetector ─────────────────────

/// 路径查表 SCRFD detector：miss 返 `default`，`with_result` 链式注入；
/// `with_error(path)` 优先级最高。
pub struct FakeFaceDetector {
    results: Mutex<HashMap<Utf8PathBuf, Vec<FaceDetection>>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: Vec<FaceDetection>,
}

impl FakeFaceDetector {
    #[must_use]
    pub fn new(default: Vec<FaceDetection>) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, faces: Vec<FaceDetection>) -> Self {
        self.results.lock().insert(path.into(), faces);
        self
    }

    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeFaceDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeFaceDetector")
            .field("default_count", &self.default.len())
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl FaceDetector for FakeFaceDetector {
    fn detect_faces(&self, path: &Utf8Path, _image_bytes: &[u8]) -> io::Result<Vec<FaceDetection>> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeFaceDetector: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .cloned()
            .unwrap_or_else(|| self.default.clone()))
    }
}

// ───────────────────── FakeFaceEmbedder ─────────────────────

/// 路径查表 `MobileFaceNet`：embedding 默认 `default_embedding`，可路径级覆盖 + Err 注入。
pub struct FakeFaceEmbedder {
    results: Mutex<HashMap<Utf8PathBuf, [f32; 128]>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: [f32; 128],
}

impl FakeFaceEmbedder {
    #[must_use]
    pub fn new(default: [f32; 128]) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, embedding: [f32; 128]) -> Self {
        self.results.lock().insert(path.into(), embedding);
        self
    }

    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeFaceEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeFaceEmbedder")
            .field("default_dim", &self.default.len())
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl FaceEmbedder for FakeFaceEmbedder {
    fn embed_face(&self, path: &Utf8Path, _aligned: &image::RgbImage) -> io::Result<[f32; 128]> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeFaceEmbedder: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .copied()
            .unwrap_or(self.default))
    }
}

// ───────────────────── FakeFaceMeshDetector ─────────────────────

/// 路径查表 `FaceMesh`：默认 468 点全 `[0,0,0]`，可路径级覆盖 + Err 注入。
pub struct FakeFaceMeshDetector {
    results: Mutex<HashMap<Utf8PathBuf, Vec<[f32; 3]>>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: Vec<[f32; 3]>,
}

impl FakeFaceMeshDetector {
    #[must_use]
    pub fn new(default: Vec<[f32; 3]>) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, mesh: Vec<[f32; 3]>) -> Self {
        self.results.lock().insert(path.into(), mesh);
        self
    }

    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeFaceMeshDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeFaceMeshDetector")
            .field("default_len", &self.default.len())
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl FaceMeshDetector for FakeFaceMeshDetector {
    fn detect_mesh(
        &self,
        path: &Utf8Path,
        _face_crop: &image::RgbImage,
    ) -> io::Result<Vec<[f32; 3]>> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeFaceMeshDetector: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .cloned()
            .unwrap_or_else(|| self.default.clone()))
    }
}

// ───────────────────── FakeEyeStateClassifier ─────────────────────

/// 路径查表眼态分类：闭眼概率默认 `default`，可路径级覆盖 + Err 注入。
pub struct FakeEyeStateClassifier {
    results: Mutex<HashMap<Utf8PathBuf, f32>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: f32,
}

impl FakeEyeStateClassifier {
    #[must_use]
    pub fn new(default: f32) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, prob: f32) -> Self {
        self.results.lock().insert(path.into(), prob);
        self
    }

    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeEyeStateClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeEyeStateClassifier")
            .field("default", &self.default)
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl EyeStateClassifier for FakeEyeStateClassifier {
    fn classify_eye(&self, path: &Utf8Path, _eye_crop: &image::RgbImage) -> io::Result<f32> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeEyeStateClassifier: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .copied()
            .unwrap_or(self.default))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_rgb() -> image::RgbImage {
        image::RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 0]))
    }

    fn sample_detection() -> FaceDetection {
        FaceDetection {
            bbox: [0.0, 0.0, 10.0, 10.0],
            score: 0.9,
            landmarks_5pt: [[1.0, 1.0]; 5],
        }
    }

    #[test]
    fn fake_face_detector_returns_default_on_miss() {
        let d = FakeFaceDetector::new(vec![sample_detection()]);
        let faces = d.detect_faces(Utf8Path::new("/x.jpg"), b"").unwrap();
        assert_eq!(faces.len(), 1);
    }

    #[test]
    fn fake_face_detector_returns_explicit_result_then_error() {
        let d = FakeFaceDetector::new(Vec::new())
            .with_result("/a.jpg", vec![sample_detection(), sample_detection()])
            .with_error("/e.jpg");
        let faces = d.detect_faces(Utf8Path::new("/a.jpg"), b"").unwrap();
        assert_eq!(faces.len(), 2);
        let e = d.detect_faces(Utf8Path::new("/e.jpg"), b"").unwrap_err();
        assert!(e.to_string().contains("injected error"));
    }

    #[test]
    fn fake_face_detector_debug_shows_counts() {
        let d = FakeFaceDetector::new(vec![sample_detection()])
            .with_result("/a", vec![sample_detection()])
            .with_error("/b");
        let s = format!("{d:?}");
        assert!(s.contains("default_count: 1"), "got: {s}");
        assert!(s.contains("results_count: 1"), "got: {s}");
        assert!(s.contains("errors_count: 1"), "got: {s}");
    }

    #[test]
    fn fake_face_embedder_path_resolution_and_error_precedence() {
        let zero = [0.0; 128];
        let mut one = [0.0; 128];
        one[0] = 1.0;
        let d = FakeFaceEmbedder::new(zero)
            .with_result("/a", one)
            .with_error("/e");
        let img = tiny_rgb();
        assert!(
            d.embed_face(Utf8Path::new("/miss"), &img).unwrap()[0].abs() < f32::EPSILON,
            "miss → default 0"
        );
        assert!(
            (d.embed_face(Utf8Path::new("/a"), &img).unwrap()[0] - 1.0).abs() < f32::EPSILON,
            "hit → with_result 1"
        );
        let err = d.embed_face(Utf8Path::new("/e"), &img).unwrap_err();
        assert!(err.to_string().contains("injected error"));
    }

    #[test]
    fn fake_face_embedder_debug_shows_counts() {
        let d = FakeFaceEmbedder::new([0.0; 128])
            .with_result("/a", [0.0; 128])
            .with_error("/b");
        let s = format!("{d:?}");
        assert!(s.contains("results_count: 1"), "got: {s}");
        assert!(s.contains("errors_count: 1"), "got: {s}");
    }

    #[test]
    fn fake_face_mesh_path_resolution_and_error_precedence() {
        let default_mesh = vec![[0.0; 3]; 468];
        let custom_mesh = vec![[1.0; 3]; 468];
        let d = FakeFaceMeshDetector::new(default_mesh)
            .with_result("/a", custom_mesh)
            .with_error("/e");
        let img = tiny_rgb();
        let m = d.detect_mesh(Utf8Path::new("/miss"), &img).unwrap();
        assert!(m[0].iter().all(|v| v.abs() < f32::EPSILON));
        let m = d.detect_mesh(Utf8Path::new("/a"), &img).unwrap();
        assert!(m[0].iter().all(|v| (v - 1.0).abs() < f32::EPSILON));
        let err = d.detect_mesh(Utf8Path::new("/e"), &img).unwrap_err();
        assert!(err.to_string().contains("injected error"));
    }

    #[test]
    fn fake_face_mesh_debug_shows_counts() {
        let d = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468])
            .with_result("/a", vec![[1.0; 3]; 468])
            .with_error("/b");
        let s = format!("{d:?}");
        assert!(s.contains("default_len: 468"), "got: {s}");
        assert!(s.contains("results_count: 1"), "got: {s}");
        assert!(s.contains("errors_count: 1"), "got: {s}");
    }

    #[test]
    fn fake_eye_state_path_resolution_and_error_precedence() {
        let d = FakeEyeStateClassifier::new(0.2)
            .with_result("/closed", 0.9)
            .with_error("/e");
        let img = tiny_rgb();
        assert!((d.classify_eye(Utf8Path::new("/miss"), &img).unwrap() - 0.2).abs() < f32::EPSILON);
        assert!(
            (d.classify_eye(Utf8Path::new("/closed"), &img).unwrap() - 0.9).abs() < f32::EPSILON
        );
        let err = d.classify_eye(Utf8Path::new("/e"), &img).unwrap_err();
        assert!(err.to_string().contains("injected error"));
    }

    #[test]
    fn fake_eye_state_debug_shows_counts() {
        let d = FakeEyeStateClassifier::new(0.5)
            .with_result("/a", 0.9)
            .with_error("/b");
        let s = format!("{d:?}");
        assert!(s.contains("default: 0.5"), "got: {s}");
        assert!(s.contains("results_count: 1"), "got: {s}");
        assert!(s.contains("errors_count: 1"), "got: {s}");
    }
}
