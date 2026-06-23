//! tract-onnx 实现 `FaceDetector`：跑 SCRFD-500M-bn-kps 输出人脸 bbox + 5 点关键点。
//!
//! 设计：
//! - preprocess letterbox 到 640×640（保持长宽比 + 灰色 padding）；scale + padding
//!   meta 与 input 一起透传给 `RawScrfd::run`，无共享可变状态保证 `Send + Sync` 真并发安全
//! - 真实 model.run + 三 stride anchor 解码 + NMS 在 `tract_scrfd_real.rs`
//!   （ignore-regex 排除整文件，CI 无 ONNX 模型不可触发）
//! - 单元测试用 `Vec<FaceDetection>` 直返的 stub `RawScrfd` 验装配 + preprocess

use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_scrfd_real::{ScaleMeta, TractRawScrfd, load_runnable};
use crate::usecases::config::FaceConfig;
use crate::usecases::face::{FaceDetection, FaceDetector};

pub(crate) type ScrfdModel = Arc<TypedRunnableModel>;

pub(crate) const INPUT_SIDE: u32 = 640;

pub(crate) trait RawScrfd: Send + Sync {
    /// 接预处理 NCHW `[1, 3, 640, 640]` f32 + letterbox meta；返已解码 + NMS 的人脸列表。
    /// trait 一步返结果（不暴露 raw tensor），让 stub 简单，anchor 算法集中真实路径。
    ///
    /// **`meta` 随 input 透传**：旧实现把 meta 写到 `Arc<Mutex<Option<ScaleMeta>>>`
    /// 共享可变状态，并发 `detect_faces` 时线程 B 的 meta 会覆盖线程 A 的，让 A 用 B 的
    /// scale/pad 逆映射坐标输出错框（违反 `FaceDetector: Send+Sync` 契约）。
    ///
    /// # Errors
    ///
    /// 模型推理失败、输出维度异常或 anchor 解码失败时返回 `Err`。
    fn run(&self, input: Tensor, meta: ScaleMeta) -> io::Result<Vec<FaceDetection>>;
}

pub struct TractScrfdDetector {
    cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发：旧 Mutex<Option<...>> 让每次
    // detect_faces 都加锁，rayon 并行评分时所有 worker 串行化在同一把锁上。
    raw: OnceLock<Box<dyn RawScrfd>>,
    // 仅 load 阶段互斥：35 worker 同时进 ensure_raw 时若都各自调 load_runnable，
    // 250 MB SCRFD model parse + tract optimize 会被并行执行 N 次（OnceLock::set
    // race 只有 1 个生效，其它 N-1 个 worker 已浪费了 load 时间）。double-checked
    // locking 让 load 只跑 1 次；inference 仍走 OnceLock::get 无锁。
    load_lock: Mutex<()>,
}

impl std::fmt::Debug for TractScrfdDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractScrfdDetector")
            .field("scrfd_model_path", &self.cfg.scrfd_model_path)
            .field("loaded", &self.raw.get().is_some())
            .field("load_lock", &self.load_lock)
            .finish()
    }
}

impl TractScrfdDetector {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawScrfd>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(raw);
        Self {
            cfg,
            raw: cell,
            load_lock: Mutex::new(()),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn ensure_raw(&self) -> io::Result<&dyn RawScrfd> {
        // 快路径：已 load 直接 OnceLock::get 无锁返。
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        // 慢路径：拿 load_lock 串行 load；双查避免 N worker 都 load 一次。
        let _guard = self.load_lock.lock();
        if let Some(r) = self.raw.get() {
            return Ok(r.as_ref());
        }
        let model = load_runnable(Path::new(&self.cfg.scrfd_model_path))?;
        let boxed: Box<dyn RawScrfd> = Box::new(TractRawScrfd {
            model,
            score_threshold: self.cfg.scrfd_score_threshold,
            nms_iou: self.cfg.scrfd_nms_iou,
        });
        let _ = self.raw.set(boxed);
        Ok(self
            .raw
            .get()
            .expect("OnceLock set by self under load_lock")
            .as_ref())
    }
}

impl FaceDetector for TractScrfdDetector {
    fn detect_faces(&self, _path: &Utf8Path, image_bytes: &[u8]) -> io::Result<Vec<FaceDetection>> {
        let raw = self.ensure_raw()?;
        let (input, meta) = preprocess(image_bytes)?;
        raw.run(input, meta)
    }
}

/// `scrfd_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `scrfd_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_scrfd_detector(cfg: &FaceConfig) -> io::Result<Box<dyn FaceDetector>> {
    if cfg.scrfd_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.scrfd_model_path is empty; set TIDYMEDIA_FACE_SCRFD_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractScrfdDetector {
        cfg: cfg.clone(),
        raw: OnceLock::new(),
        load_lock: Mutex::new(()),
    }))
}

/// Letterbox 把图像 resize 到 640×640（保持长宽比，灰色 128 padding），同时记录
/// scale + padding 给 postprocess 逆映射用。返 `(NCHW tensor, ScaleMeta)`。
pub(crate) fn preprocess(bytes: &[u8]) -> io::Result<(Tensor, ScaleMeta)> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")))?;
    let rgb = img.to_rgb8();
    let (orig_w, orig_h) = (rgb.width(), rgb.height());
    // INPUT_SIDE 是编译期 const 640，f32 字面量直接表达 letterbox scale 计算用值，
    // 替 `f32::from(u16::try_from(640).expect("640 fits u16"))` 的运行时不可达 try_from。
    let side_f: f32 = 640.0;
    #[expect(clippy::cast_precision_loss, reason = "图像维度 ≤ 65535，f32 精度够")]
    let scale = (side_f / orig_w as f32).min(side_f / orig_h as f32);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scale ∈ (0, 1]，乘原维度后截断到 u32 安全"
    )]
    let new_w = (orig_w as f32 * scale).round() as u32;
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "同上"
    )]
    let new_h = (orig_h as f32 * scale).round() as u32;
    let resized = image::imageops::resize(
        &rgb,
        new_w.max(1),
        new_h.max(1),
        image::imageops::FilterType::Triangle,
    );
    let pad_x = (INPUT_SIDE - new_w.min(INPUT_SIDE)) / 2;
    let pad_y = (INPUT_SIDE - new_h.min(INPUT_SIDE)) / 2;
    let mut canvas =
        image::RgbImage::from_pixel(INPUT_SIDE, INPUT_SIDE, image::Rgb([128, 128, 128]));
    image::imageops::overlay(&mut canvas, &resized, i64::from(pad_x), i64::from(pad_y));

    let side = INPUT_SIDE as usize;
    let plane = side * side;
    let mut chw = vec![0.0_f32; 3 * plane];
    for (idx, px) in canvas.pixels().enumerate() {
        let y = idx / side;
        let x = idx % side;
        for ch in 0..3 {
            // SCRFD 训练: (v - 127.5) / 128 → 近 [-1, 1]
            chw[ch * plane + y * side + x] = (f32::from(px.0[ch]) - 127.5) / 128.0;
        }
    }
    // 同 mobilefacenet.preprocess：const 形状下 Err arm 不可达，map_err 让 caller 经 ? 传播。
    let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), chw)
        .map_err(|e| io::Error::other(format!("SCRFD preprocess shape: {e}")))?
        .into_tensor();
    #[expect(clippy::cast_precision_loss, reason = "pad ≤ 640，f32 精确")]
    let meta = ScaleMeta {
        scale,
        pad_x: pad_x as f32,
        pad_y: pad_y as f32,
    };
    Ok((tensor, meta))
}

#[cfg(test)]
#[path = "tract_scrfd_tests.rs"]
mod tests;
