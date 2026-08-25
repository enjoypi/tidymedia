//! 测试用 4 个 face Gateway 实现：路径查表 + Err 注入。与 `FakeTextDetector` 同套路。

use std::collections::{HashMap, HashSet};
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::Mutex;

use crate::usecases::face::{
    EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector,
};

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
#[path = "fake_tests.rs"]
mod tests;
