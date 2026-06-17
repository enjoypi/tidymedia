//! 人脸质量评分 Output Port：4 个 Gateway trait（检测/嵌入/网格/眼态）+ 跨边界
//! DTO [`FaceDetection`]。`cull` use case 通过这些接口调用人脸推理，不知道具体
//! 推理后端（tract-onnx / Candle / ort 等可替换）。
//!
//! 设计要点：
//! - **职责单一**：每个模型一个 trait，按 SOLID 接口隔离；只接受调用方真正
//!   需要的入参（SCRFD 接原始图字节，自己解码；其余三个接 SCRFD 检测后的
//!   人脸/眼部 crop `image::RgbImage`，避免重复解码）
//! - **`Send + Sync + Debug`**：与 `TextDetector` 同约定，可放 `Arc<dyn _>` 共享
//! - **path 入参**：`&Utf8Path` 作为 decision context（日志键、Fake 注入键），
//!   真实实现忽略 path 内容
//! - **位置**：按 Clean Architecture，Output Port 定义在 use case 层；具体实现
//!   （`tract_*.rs` + Fake）在 `adapters::face`

use std::io;

use camino::Utf8Path;

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

/// 人脸嵌入 Gateway：输入对齐后的 112×112 RGB，输出 L2-normalized 128 维向量。
///
/// 维度选 128：`foamliu/MobileFaceNet` 训练规格（论文标准 embedding 维），与官方
/// `InsightFace` 512 维变体不同但接口契约一致——dim 不进对外 API 名只进内部常量。
pub trait FaceEmbedder: Send + Sync + std::fmt::Debug {
    /// 对 `aligned` 跑 `MobileFaceNet` 返 128 维 embedding（L2 后）。
    ///
    /// # Errors
    ///
    /// 模型推理失败或路径级注入错误时返回 `Err`。
    fn embed_face(&self, path: &Utf8Path, aligned: &image::RgbImage) -> io::Result<[f32; 128]>;
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
