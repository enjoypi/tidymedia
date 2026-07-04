//! `DetectorFactory` Port：装配 cull / move-text-shot 需要的推理 detector。
//!
//! 与 [`crate::entities::backend::factory::BackendFactory`] 对偶——backend 是
//! 基础设施 Gateway 抽象故落在 `entities/`，detector 是推理 Port 故落在 `usecases/`
//! （CLAUDE.md「新增 Output Port trait MUST 落到内层」）。具体实现
//! [`crate::frameworks::detector::DefaultDetectorFactory`] 在 frameworks 层装配
//! 决定「用哪个真实 detector」，adapters 层的 dispatch 只消费 trait method。
//!
//! 5 个 method 语义正交（4 face + 1 text），单一 trait 而非拆成 5 个是 dispatch
//! 单点注入 + `BackendFactory` 对偶哲学的折中——两个 Cull/MoveTextShot 命令按需
//! 触发其中的子集，未触发的 detector 不被 build（保留旧「按用例装配」的懒行为）。

use std::io;

use crate::usecases::face::{EyeStateClassifier, FaceDetector, FaceEmbedder, FaceMeshDetector};
use crate::usecases::ocr::TextDetector;

/// 推理 detector 装配 Port。每个方法在 dispatch 需要时才被调用，避免
/// Copy/Find/Move 命令误装 ONNX 模型。
pub trait DetectorFactory: Send + Sync {
    /// `SCRFD` 人脸检测器。
    ///
    /// # Errors
    ///
    /// ONNX 模型路径为空 / 加载失败时返 `io::Error`。
    fn build_face_detector(&self) -> io::Result<Box<dyn FaceDetector>>;

    /// `MobileFaceNet` 128 维 embedding。
    ///
    /// # Errors
    ///
    /// 同 [`Self::build_face_detector`]。
    fn build_face_embedder(&self) -> io::Result<Box<dyn FaceEmbedder>>;

    /// `MediaPipe` `FaceMesh` 192×192。
    ///
    /// # Errors
    ///
    /// 同 [`Self::build_face_detector`]。
    fn build_face_mesh(&self) -> io::Result<Box<dyn FaceMeshDetector>>;

    /// `YOLOv8` `EyeState` 分类器。
    ///
    /// # Errors
    ///
    /// 同 [`Self::build_face_detector`]。
    fn build_eye_state_classifier(&self) -> io::Result<Box<dyn EyeStateClassifier>>;

    /// `PaddleOCR` `DBNet` 文本检测。
    ///
    /// # Errors
    ///
    /// 同 [`Self::build_face_detector`]。
    fn build_text_detector(&self) -> io::Result<Box<dyn TextDetector>>;
}
