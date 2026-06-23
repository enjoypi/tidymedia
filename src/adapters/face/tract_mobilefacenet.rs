//! tract-onnx 实现 `FaceEmbedder`：跑 `MobileFaceNet` 输出 128 维 L2-normalized embedding。
//! 维度按 `foamliu/MobileFaceNet` 训练规格定（论文标准 128 维）；官方 `InsightFace`
//! 512 维变体未启用——切换时同步改 `EMBED_DIM` 与 `FaceEmbedder` 接口签名。
//!
//! 设计与 `tract_dbnet` 同构：内部 `RawFacenet` trait 隔离真实 `model.run`，
//! 单测注入 `ConstRaw` 验前/后处理，真实加载在 `_real.rs` 走 ignore-regex 排除。

use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_mobilefacenet_real::load_runnable;
use crate::usecases::config::FaceConfig;
use crate::usecases::face::FaceEmbedder;

/// 已优化的 `MobileFaceNet` 推理图。
pub(crate) type FacenetModel = Arc<TypedRunnableModel>;

const EMBED_DIM: usize = 128;
const INPUT_SIDE: u32 = 112;

/// 把模型加载与单张推理拆开注入，让前/后处理可独立单测。
pub(crate) trait RawFacenet: Send + Sync {
    /// 接预处理 NCHW `[1, 3, 112, 112]` f32；返 `[1, 128]` f32 embedding（未 L2）。
    ///
    /// # Errors
    ///
    /// 模型推理失败或输出维度不符时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

struct TractRawFacenet {
    model: FacenetModel,
}

impl RawFacenet for TractRawFacenet {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn run(&self, input: Tensor) -> io::Result<Tensor> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract MobileFaceNet run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("tract MobileFaceNet returned no output tensor"))?;
        Ok(first.into_tensor())
    }
}

pub struct TractFacenetEmbedder {
    cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    raw: OnceLock<Box<dyn RawFacenet>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    load_lock: Mutex<()>,
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

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<&dyn RawFacenet> {
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let _guard = self.load_lock.lock();
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let model = load_runnable(Path::new(&self.cfg.facenet_model_path))?;
        let boxed: Box<dyn RawFacenet> = Box::new(TractRawFacenet { model });
        let _ = self.raw.set(boxed);
        Ok(self
            .raw
            .get()
            .expect("OnceLock set by self under load_lock")
            .as_ref())
    }
}

impl FaceEmbedder for TractFacenetEmbedder {
    fn embed_face(&self, _path: &Utf8Path, aligned: &image::RgbImage) -> io::Result<[f32; 128]> {
        let raw = self.ensure_raw()?;
        let input = preprocess(aligned)?;
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

/// 输入 112×112 RGB → `[-1, 1]` 归一化 NCHW `[1, 3, 112, 112]` f32。
/// 非 112×112 入参用 Triangle filter 强制 resize（与 `ArcFace` 训练一致）。
///
/// # Errors
///
/// `Array4::from_shape_vec` 形状失配返 Err（const 形状下数学上不可达，留 ? 让未来动态 shape 兼容）。
pub(crate) fn preprocess(img: &image::RgbImage) -> io::Result<Tensor> {
    // 已对齐 INPUT_SIDE 时 Cow::Borrowed 零拷贝；P0 §3 借用参数避免不必要克隆。
    let resized: std::borrow::Cow<'_, image::RgbImage> =
        if img.width() == INPUT_SIDE && img.height() == INPUT_SIDE {
            std::borrow::Cow::Borrowed(img)
        } else {
            std::borrow::Cow::Owned(image::imageops::resize(
                img,
                INPUT_SIDE,
                INPUT_SIDE,
                image::imageops::FilterType::Triangle,
            ))
        };

    let side = INPUT_SIDE as usize;
    let plane = side * side;
    let mut chw = vec![0.0_f32; 3 * plane];
    for (idx, px) in resized.pixels().enumerate() {
        let y = idx / side;
        let x = idx % side;
        for ch in 0..3 {
            // MobileFaceNet 训练标准：(v - 127.5) / 127.5 → [-1, 1]
            let v = (f32::from(px.0[ch]) - 127.5) / 127.5;
            chw[ch * plane + y * side + x] = v;
        }
    }
    // 形状 const、vec 长度数学上严格匹配 → Err arm 实际不可达，但 CLAUDE.md 要求
    // tract 模型用 map_err 让 Err 可传播（避免 .expect 掩盖 API 形态），同时未来若
    // INPUT_SIDE 变量化（动态 shape）自动兼容。caller 经 ? 上抛由上游 record_failure 计入。
    tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .map_err(|e| io::Error::other(format!("facenet preprocess shape: {e}")))
        .map(IntoTensor::into_tensor)
}

/// 取 `[1, 128]` embedding 并 L2 normalize → `[f32; 128]`。
///
/// 严格匹配 `EMBED_DIM`：不允许截断/补零。错配模型（如 `InsightFace` 512 维 `w600k_r50.onnx`）
/// 输出 `slice.len()=512` 时旧 `< EMBED_DIM` 守卫放行让 `copy_from_slice` 截取前 128 维做 L2，
/// embedding 空间与正确 128 维不兼容 → cosine 比较产生随机聚类（同人多张照片判作不同身份）。
pub(crate) fn decode(output: &Tensor) -> io::Result<[f32; 128]> {
    let cast = output
        .cast_to::<f32>()
        .map_err(|e| io::Error::other(format!("facenet output not f32-castable: {e}")))?;
    let view = cast.view();
    let slice = view
        .as_slice::<f32>()
        .map_err(|e| io::Error::other(format!("facenet output slice: {e}")))?;
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
