//! 真实 SCRFD-500M-bn-kps ONNX 加载 + 三 stride anchor 解码 + NMS。
//! 走 `--ignore-filename-regex='_real\.rs$'` 排除整文件：anchor 解码 + NMS 算法
//! 完整实现需要按真实 ONNX 输出 layout 调参（plan F 节风险 #1 已明示需 e2e 真跑），
//! 算法主体放此处与真 `model.run` 一起，简化生产/测试分界。

use std::io;
use std::path::Path;

use tract_onnx::prelude::*;

use super::tract_scrfd::{INPUT_SIDE, RawScrfd, ScrfdModel};
use crate::usecases::face::FaceDetection;

/// 读 ONNX → optimized → runnable。
///
/// # Errors
///
/// 文件不存在、ONNX 解析、优化或形状推导失败时返回 `Err`。
pub(crate) fn load_runnable(path: &Path) -> io::Result<ScrfdModel> {
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| io::Error::other(format!("load SCRFD ONNX {}: {e}", path.display())))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize SCRFD model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make SCRFD runnable: {e}")))?;
    Ok(model)
}

/// 真实 SCRFD detector：跑 model + anchor 解码 + NMS。`meta` 随 input 透传，
/// 不持有共享可变状态（避免并发 `detect_faces` 时 meta 互相覆盖产生错框）。
pub(crate) struct TractRawScrfd {
    pub(crate) model: ScrfdModel,
    pub(crate) score_threshold: f32,
    pub(crate) nms_iou: f32,
}

/// preprocess 阶段记录 letterbox scale + padding，让 postprocess 把 bbox 坐标
/// 逆映射回原图。Copy + 按值传递避免共享可变状态。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScaleMeta {
    pub(crate) scale: f32,
    pub(crate) pad_x: f32,
    pub(crate) pad_y: f32,
}

impl RawScrfd for TractRawScrfd {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn run(&self, input: Tensor, meta: ScaleMeta) -> io::Result<Vec<FaceDetection>> {
        let outputs = self
            .model
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| io::Error::other(format!("tract SCRFD run failed: {e}")))?;
        decode_outputs(&outputs, self.score_threshold, self.nms_iou, &meta)
    }
}

/// SCRFD-500M-bn-kps 三 stride（8/16/32）输出解 anchor + NMS。
/// 输出 layout 假设：`[score_8, bbox_8, kps_8, score_16, bbox_16, kps_16, score_32, bbox_32, kps_32]`，
/// 每个 score `[1, A, 1]`、bbox `[1, A, 4]`、kps `[1, A, 10]`，`A=grid_h*grid_w*num_anchors`。
#[cfg_attr(coverage_nightly, coverage(off))]
fn decode_outputs(
    outputs: &[TValue],
    score_threshold: f32,
    nms_iou: f32,
    meta: &ScaleMeta,
) -> io::Result<Vec<FaceDetection>> {
    const STRIDES: [u32; 3] = [8, 16, 32];
    const NUM_ANCHORS: u32 = 2;

    if outputs.len() < 9 {
        return Err(io::Error::other(format!(
            "SCRFD expects 9 output tensors, got {}",
            outputs.len()
        )));
    }
    let mut detections = Vec::new();
    for (i, &stride) in STRIDES.iter().enumerate() {
        let score_cow = outputs[i * 3].cast_to::<f32>().map_err(io::Error::other)?;
        let bbox_cow = outputs[i * 3 + 1]
            .cast_to::<f32>()
            .map_err(io::Error::other)?;
        let kps_cow = outputs[i * 3 + 2]
            .cast_to::<f32>()
            .map_err(io::Error::other)?;
        let score_view = score_cow.view();
        let bbox_view = bbox_cow.view();
        let kps_view = kps_cow.view();
        let score = score_view
            .as_slice::<f32>()
            .map_err(|e| io::Error::other(format!("SCRFD score slice: {e}")))?;
        let bbox = bbox_view
            .as_slice::<f32>()
            .map_err(|e| io::Error::other(format!("SCRFD bbox slice: {e}")))?;
        let kps = kps_view
            .as_slice::<f32>()
            .map_err(|e| io::Error::other(format!("SCRFD kps slice: {e}")))?;
        let grid_side = INPUT_SIDE / stride;
        for gy in 0..grid_side {
            for gx in 0..grid_side {
                for a in 0..NUM_ANCHORS {
                    let idx = ((gy * grid_side + gx) * NUM_ANCHORS + a) as usize;
                    if idx >= score.len() {
                        continue;
                    }
                    let s = score[idx];
                    if s < score_threshold {
                        continue;
                    }
                    let bo = idx * 4;
                    let ko = idx * 10;
                    if bo + 4 > bbox.len() || ko + 10 > kps.len() {
                        continue;
                    }
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "stride/grid index ≤ 80，f32 精度够用"
                    )]
                    let cx = (gx as f32 + 0.5) * stride as f32;
                    #[expect(clippy::cast_precision_loss, reason = "同上")]
                    let cy = (gy as f32 + 0.5) * stride as f32;
                    #[expect(clippy::cast_precision_loss, reason = "stride 8/16/32 → f32 精确")]
                    let stride_f = stride as f32;
                    let x1 = cx - bbox[bo] * stride_f;
                    let y1 = cy - bbox[bo + 1] * stride_f;
                    let x2 = cx + bbox[bo + 2] * stride_f;
                    let y2 = cy + bbox[bo + 3] * stride_f;
                    let mut landmarks = [[0.0_f32; 2]; 5];
                    for (k, lm) in landmarks.iter_mut().enumerate() {
                        lm[0] = cx + kps[ko + k * 2] * stride_f;
                        lm[1] = cy + kps[ko + k * 2 + 1] * stride_f;
                    }
                    detections.push(FaceDetection {
                        bbox: [
                            (x1 - meta.pad_x) / meta.scale,
                            (y1 - meta.pad_y) / meta.scale,
                            (x2 - meta.pad_x) / meta.scale,
                            (y2 - meta.pad_y) / meta.scale,
                        ],
                        score: s,
                        landmarks_5pt: landmarks.map(|p| {
                            [
                                (p[0] - meta.pad_x) / meta.scale,
                                (p[1] - meta.pad_y) / meta.scale,
                            ]
                        }),
                    });
                }
            }
        }
    }
    Ok(nms(detections, nms_iou))
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn nms(mut dets: Vec<FaceDetection>, iou_threshold: f32) -> Vec<FaceDetection> {
    dets.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<FaceDetection> = Vec::new();
    for d in dets {
        if kept.iter().any(|k| iou(&k.bbox, &d.bbox) > iou_threshold) {
            continue;
        }
        kept.push(d);
    }
    kept
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let w = (x2 - x1).max(0.0);
    let h = (y2 - y1).max(0.0);
    let inter = w * h;
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= f32::EPSILON {
        0.0
    } else {
        inter / union
    }
}
