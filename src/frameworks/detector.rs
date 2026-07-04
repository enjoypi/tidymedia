//! [`DetectorFactory`] 默认装配：调 `adapters/{face,ocr}` 的 tract-onnx `build_*`
//! 生成生产 detector。此模块把「决定用哪个具体 detector 实现」的选择集中到
//! Frameworks 层（Robert Martin: "main is where all the details go"），
//! adapters 层的 dispatch 恢复只调 `UseCase` 的薄 Controller 语义。

use std::io;

use crate::adapters;
use crate::usecases::config::config;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::face::{EyeStateClassifier, FaceDetector, FaceEmbedder, FaceMeshDetector};
use crate::usecases::ocr::TextDetector;

/// 生产 [`DetectorFactory`]：每次调用读全局 `config()` 装配对应 detector。
/// 单例 unit struct，`tidy_with` / `tidy` / mobile FFI 各调用点持 `&Self`。
#[derive(Debug, Default)]
pub struct DefaultDetectorFactory;

impl DetectorFactory for DefaultDetectorFactory {
    fn build_face_detector(&self) -> io::Result<Box<dyn FaceDetector>> {
        adapters::face::build_scrfd_detector(&config().backend.face)
    }

    fn build_face_embedder(&self) -> io::Result<Box<dyn FaceEmbedder>> {
        adapters::face::build_facenet_embedder(&config().backend.face)
    }

    fn build_face_mesh(&self) -> io::Result<Box<dyn FaceMeshDetector>> {
        adapters::face::build_facemesh(&config().backend.face)
    }

    fn build_eye_state_classifier(&self) -> io::Result<Box<dyn EyeStateClassifier>> {
        adapters::face::build_eyestate_classifier(&config().backend.face)
    }

    fn build_text_detector(&self) -> io::Result<Box<dyn TextDetector>> {
        adapters::ocr::build_detector(&config().backend.ocr)
    }
}
