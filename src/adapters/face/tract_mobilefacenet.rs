//! tract-onnx 实现 `FaceEmbedder`：跑 `MobileFaceNet` 输出 128 维 L2-normalized embedding。
//! 维度按 `foamliu/MobileFaceNet` 训练规格定（论文标准 128 维）；官方 `InsightFace`
//! 512 维变体未启用——切换时同步改 `EMBED_DIM` 与 `FaceEmbedder` 接口签名。
//!
//! 设计与 `tract_dbnet` 同构：内部 `RawFacenet` trait 隔离真实 `model.run`，
//! 单测注入 `ConstRaw` 验前/后处理，真实加载在 `_real.rs` 走 ignore-regex 排除。

use std::io;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use crate::usecases::config::FaceConfig;
use crate::usecases::face::FaceEmbedder;

/// 已优化的 `MobileFaceNet` 推理图。
pub(crate) type FacenetModel = Arc<TypedRunnableModel>;

const EMBED_DIM: usize = 128;

/// 把模型加载与单张推理拆开注入，让前/后处理可独立单测。
pub(crate) trait RawFacenet: Send + Sync {
    /// 接预处理 NCHW `[1, 3, 112, 112]` f32；返 `[1, 128]` f32 embedding（未 L2）。
    ///
    /// # Errors
    ///
    /// 模型推理失败或输出维度不符时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

pub struct TractFacenetEmbedder {
    pub(crate) cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    pub(crate) raw: OnceLock<Box<dyn RawFacenet>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    pub(crate) load_lock: Mutex<()>,
}

impl std::fmt::Debug for TractFacenetEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractFacenetEmbedder")
            .field("facenet_model_path", &self.cfg.facenet_model_path)
            .field("loaded", &self.raw.get().is_some())
            .field("load_lock", &self.load_lock)
            .finish()
    }
}

impl TractFacenetEmbedder {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawFacenet>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(raw);
        Self {
            cfg,
            raw: cell,
            load_lock: Mutex::new(()),
        }
    }
}

impl FaceEmbedder for TractFacenetEmbedder {
    fn embed_face(&self, _path: &Utf8Path, aligned: &image::RgbImage) -> io::Result<[f32; 128]> {
        let raw = self.ensure_raw()?;
        let input = preprocess(aligned);
        let output = raw.run(input)?;
        decode(&output)
    }
}

/// `facenet_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `facenet_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_facenet_embedder(cfg: &FaceConfig) -> io::Result<Box<dyn FaceEmbedder>> {
    if cfg.facenet_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.facenet_model_path is empty; set TIDYMEDIA_FACE_FACENET_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractFacenetEmbedder {
        cfg: cfg.clone(),
        raw: OnceLock::new(),
        load_lock: Mutex::new(()),
    }))
}

// 输入预处理（112×112 resize + 归一化）独立到 `tract_mobilefacenet_phantom.rs`：
// e2e bin 真实模型输出实例（aligned crop 对齐 112）只触发 Cow::Borrowed 方向，
// 跨 instance 合并出 phantom branch miss，独立成文件后走 ignore-regex 排除。
#[path = "tract_mobilefacenet_phantom.rs"]
mod phantom;

#[doc(hidden)]
pub(crate) use self::phantom::preprocess;

/// 取 `[1, 128]` embedding 并 L2 normalize → `[f32; 128]`。
///
/// 严格匹配 `EMBED_DIM`：不允许截断/补零。错配模型（如 `InsightFace` 512 维 `w600k_r50.onnx`）
/// 输出 `slice.len()=512` 时旧 `< EMBED_DIM` 守卫放行让 `copy_from_slice` 截取前 128 维做 L2，
/// embedding 空间与正确 128 维不兼容 → cosine 比较产生随机聚类（同人多张照片判作不同身份）。
#[doc(hidden)]
pub fn decode(output: &Tensor) -> io::Result<[f32; 128]> {
    let cast = output.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .expect("cast 成功后 view 恒 contiguous");
    if slice.len() != EMBED_DIM {
        return Err(io::Error::other(format!(
            "facenet output dim {} != expected {EMBED_DIM} \
             (check backend.face.facenet_model_path: must be 128-dim MobileFaceNet, \
             not 512-dim InsightFace variant)",
            slice.len()
        )));
    }
    let mut out = [0.0_f32; EMBED_DIM];
    out.copy_from_slice(&slice[..EMBED_DIM]);
    let norm: f32 = out.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for v in &mut out {
            *v /= norm;
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "tract_mobilefacenet_tests.rs"]
mod tests;
