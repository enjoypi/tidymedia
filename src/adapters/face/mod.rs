//! 人脸推理 Adapter：4 个 Port trait 实现 + Fake。
//!
//! Port 定义见 `crate::usecases::face`（4 个 trait + `FaceDetection` DTO）。
//! 真实实现走 tract-onnx，每 trait 拆 `tract_xxx.rs`（算法主体）+ `tract_xxx_real.rs`
//! （`_real` 走 `--ignore-filename-regex` 排除整文件：CI 无 ONNX 模型不可触发）。

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
