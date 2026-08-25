//! tract-onnx 实现 `EyeStateClassifier`：跑 `YOLOv8` 眼态检测取闭眼最大置信。
//!
//! 模型源：`MichalMlodawski/open-closed-eye-detection`（`YOLOv8` 检测头，非 softmax）。
//! 输入：任意 RGB → 640×640 letterbox（灰底）→ `[0, 1]` 归一化 → NCHW `[1, 3, 640, 640]`
//! 输出：`[1, 6, 8400]` = 4 box `(cx,cy,w,h)` + 2 class conf `(open=0, closed=1)`；
//! 本实现遍历 8400 个 anchor 取 closed conf 最大值作为 blink probability。
//!
//! 注：closed 类索引按 README 描述（open=0, closed=1）；Netron 校对若反向需翻转
//! `CLOSED_CLASS_IDX`。
//!
//! `EyeStateClassifier` trait 契约（接 `eye_crop`）保留：把 eye crop 当全图送 letterbox
//! 后仍可被检测到 + 分类——比起经典 `MobileNetV3` softmax 二分类，`YOLOv8` 输出对
//! 输入尺度更鲁棒。

use std::io;
use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use crate::usecases::config::FaceConfig;
use crate::usecases::face::EyeStateClassifier;

pub(crate) type EyeStateModel = Arc<TypedRunnableModel>;

pub(crate) trait RawEyeState: Send + Sync {
    /// 接 NCHW `[1, 3, 640, 640]` f32；返 `[1, 6, anchors]` f32 `YOLOv8` 检测头输出。
    ///
    /// # Errors
    ///
    /// 模型推理失败或输出维度不符时返回 `Err`。
    fn run(&self, input: Tensor) -> io::Result<Tensor>;
}

pub struct TractEyeStateClassifier {
    pub(crate) cfg: FaceConfig,
    // OnceLock 让 lazy init 后 inference 无锁并发（同 SCRFD：旧 Mutex 串行化所有 worker）。
    pub(crate) raw: OnceLock<Box<dyn RawEyeState>>,
    // load 阶段互斥避免 N worker 重复 load model（详见 tract_scrfd.rs 同字段注释）。
    pub(crate) load_lock: Mutex<()>,
}

impl std::fmt::Debug for TractEyeStateClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TractEyeStateClassifier")
            .field("eyestate_model_path", &self.cfg.eyestate_model_path)
            .field("loaded", &self.raw.get().is_some())
            .field("load_lock", &self.load_lock)
            .finish()
    }
}

impl TractEyeStateClassifier {
    #[cfg(test)]
    pub(crate) fn with_raw(cfg: FaceConfig, raw: Box<dyn RawEyeState>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(raw);
        Self {
            cfg,
            raw: cell,
            load_lock: Mutex::new(()),
        }
    }
}

impl EyeStateClassifier for TractEyeStateClassifier {
    fn classify_eye(&self, _path: &Utf8Path, eye_crop: &image::RgbImage) -> io::Result<f32> {
        let raw = self.ensure_raw()?;
        let input = preprocess(eye_crop)?;
        let output = raw.run(input)?;
        decode(&output)
    }
}

/// `eyestate_model_path` 为空时报 `InvalidInput`。
///
/// # Errors
///
/// 当 `eyestate_model_path` 为空或模型加载失败时返回 `Err`。
pub fn build_eyestate_classifier(cfg: &FaceConfig) -> io::Result<Box<dyn EyeStateClassifier>> {
    if cfg.eyestate_model_path.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backend.face.eyestate_model_path is empty; set TIDYMEDIA_FACE_EYESTATE_MODEL or config.yaml",
        ));
    }
    Ok(Box::new(TractEyeStateClassifier {
        cfg: cfg.clone(),
        raw: OnceLock::new(),
        load_lock: Mutex::new(()),
    }))
}

// 前后处理（letterbox preprocess / YOLOv8 decode）独立到 `tract_eyestate_phantom.rs`：
// e2e bin 真实模型输出实例不触发其全部分支方向（src_h==0、错误 rank/0 anchors），
// 跨 instance 合并出 phantom branch miss，独立成文件后走 ignore-regex 排除。
#[path = "tract_eyestate_phantom.rs"]
mod phantom;

#[doc(hidden)]
pub use self::phantom::{decode, preprocess};

#[cfg(test)]
pub(crate) use self::phantom::{BOX_DIMS, CLOSED_CLASS_IDX, OUTPUT_CHANNELS};

#[cfg(test)]
#[path = "tract_eyestate_tests.rs"]
mod tests;
