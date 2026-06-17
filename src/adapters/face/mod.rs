//! 人脸质量评分 Gateway 抽象：4 个 trait（检测/嵌入/网格/眼态）让 `cull`
//! usecase 不直接依赖 tract-onnx，便于 Fake 注入测试 + 未来换推理后端。
//!
//! 设计要点：
//! - **职责单一**：每个模型一个 trait，按 SOLID 接口隔离；只接受调用方真正
//!   需要的入参（SCRFD 接原始图字节，自己解码；其余三个接 SCRFD 检测后的
//!   人脸/眼部 crop `image::RgbImage`，避免重复解码）
//! - **`Send + Sync + Debug`**：与 `TextDetector` 同约定，可放 `Arc<dyn _>` 共享
//! - **path 入参**：`&Utf8Path` 作为 decision context（日志键、Fake 注入键），
//!   真实 tract 实现忽略 path 内容
//! - **真实实现走 `_real.rs` 拆文件 + `--ignore-filename-regex` 排除整文件**：
//!   CI 无 ONNX 模型不可触发；算法（preprocess/postprocess）留主文件 lib unit 测

use std::io;

use camino::Utf8Path;

pub mod fake;

pub mod tract_eyestate;
pub mod tract_eyestate_real;
pub mod tract_facemesh;
pub mod tract_facemesh_real;
pub mod tract_mobilefacenet;
pub mod tract_mobilefacenet_real;
pub mod tract_scrfd;
pub mod tract_scrfd_real;

pub use tract_eyestate::build_eyestate_classifier;
pub use tract_facemesh::build_facemesh;
pub use tract_mobilefacenet::build_facenet_embedder;
pub use tract_scrfd::build_scrfd_detector;

/// SCRFD 单张人脸检测结果：bbox + score + 5 点关键点（左眼/右眼/鼻尖/左嘴角/右嘴角）。
/// 坐标都是原图像素坐标（preprocess 后处理已 inverse-scale 回原图）。
#[derive(Clone, Copy, Debug)]
pub struct FaceDetection {
    /// `[x1, y1, x2, y2]` 原图像素坐标。
    pub bbox: [f32; 4],
    /// 置信度 `[0, 1]`。
    pub score: f32,
    /// 5 点关键点；顺序与 `ArcFace` 模板对齐（左眼/右眼/鼻尖/左嘴角/右嘴角）。
    pub landmarks_5pt: [[f32; 2]; 5],
}

/// 人脸检测 Gateway。实现者按字节解码图像 + 跑 SCRFD + 返人脸列表。
pub trait FaceDetector: Send + Sync + std::fmt::Debug {
    /// 检测 `image_bytes` 中所有人脸。
    ///
    /// # Errors
    ///
    /// 字节解码失败、模型推理失败或路径级注入错误时返回 `Err`。
    fn detect_faces(&self, path: &Utf8Path, image_bytes: &[u8]) -> io::Result<Vec<FaceDetection>>;
}

/// 人脸嵌入 Gateway：输入对齐后的 112×112 RGB，输出 L2-normalized 512 维向量。
pub trait FaceEmbedder: Send + Sync + std::fmt::Debug {
    /// 对 `aligned` 跑 `MobileFaceNet` 返 512 维 embedding（L2 后）。
    ///
    /// # Errors
    ///
    /// 模型推理失败或路径级注入错误时返回 `Err`。
    fn embed_face(&self, path: &Utf8Path, aligned: &image::RgbImage) -> io::Result<[f32; 512]>;
}

/// `FaceMesh` Gateway：输入 192×192 人脸 crop，输出 468 个 3D 关键点（用于 EAR）。
pub trait FaceMeshDetector: Send + Sync + std::fmt::Debug {
    /// 对 `face_crop` 跑 `MediaPipe` `FaceMesh` 返 468 个 `[x, y, z]` 关键点。
    ///
    /// # Errors
    ///
    /// 模型推理失败或路径级注入错误时返回 `Err`。
    fn detect_mesh(
        &self,
        path: &Utf8Path,
        face_crop: &image::RgbImage,
    ) -> io::Result<Vec<[f32; 3]>>;
}

/// 眼部状态分类 Gateway：输入眼部裁剪图（任意尺寸内部 resize），输出闭眼概率。
pub trait EyeStateClassifier: Send + Sync + std::fmt::Debug {
    /// 对 `eye_crop` 跑 `EyeState` 二分类返闭眼概率 `[0, 1]`。
    ///
    /// # Errors
    ///
    /// 模型推理失败或路径级注入错误时返回 `Err`。
    fn classify_eye(&self, path: &Utf8Path, eye_crop: &image::RgbImage) -> io::Result<f32>;
}
