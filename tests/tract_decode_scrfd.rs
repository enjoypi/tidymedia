#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::manual_is_multiple_of,
    clippy::redundant_clone,
    clippy::single_char_pattern,
    clippy::unnecessary_trailing_comma
)]

//! SCRFD `decode_outputs` / `nms` / `iou` 的畸形与边界覆盖。
//! 这些分支（输出 < 9 张、NaN score、bbox/kps 越界、零面积 box）真模型输出
//! 恒不触发，只能经 `#[doc(hidden)] pub` 观测口在 release 实例直调。

use tract_onnx::prelude::*;

use tidymedia::{FaceDetection, ScaleMeta, decode_outputs, iou, nms};

const STRIDES: [u32; 3] = [8, 16, 32];

fn tvalue_of_f32(shape: &[usize], data: &[f32]) -> TValue {
    let arr = tract_ndarray::Array::from_shape_vec(shape, data.to_vec()).unwrap();
    let t: TValue = arr.into_tensor().into();
    t
}

fn image_for(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for p in img.pixels_mut() {
        *p = image::Rgb([90, 120, 200]);
    }
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("in-memory PNG encode");
    buf
}

/// 9 个输出张量：score/bbox/kps × 3 stride，每个 shape `[1, A, N]`。
/// `anchor` 计数与 STRIDES 的 grid 大小配对，让 score 主循环保持紧凑。
fn outputs(score_fn: impl Fn(usize) -> f32) -> Vec<TValue> {
    let mut out = Vec::new();
    for (si, _) in STRIDES.iter().enumerate() {
        let a_per_stride = 4usize;
        let score: Vec<f32> = (0..a_per_stride)
            .map(|i| score_fn(si * a_per_stride + i))
            .collect();
        let bbox: Vec<f32> = (0..a_per_stride)
            .flat_map(|_| [0.02, 0.03, 0.04, 0.05])
            .collect();
        let kps: Vec<f32> = (0..10 * a_per_stride)
            .map(|i| i as f32 * 0.001 - 0.05)
            .collect();
        out.push(tvalue_of_f32(&[1, a_per_stride, 1], &score));
        out.push(tvalue_of_f32(&[1, a_per_stride, 4], &bbox));
        out.push(tvalue_of_f32(&[1, a_per_stride, 10], &kps));
    }
    out
}

#[test]
fn decode_rejects_less_than_nine_outputs() {
    let meta = ScaleMeta {
        scale: 1.0,
        pad_x: 0.0,
        pad_y: 0.0,
    };
    let err = decode_outputs(&outputs(|_| 0.0)[..8], 0.5, 0.4, &meta).unwrap_err();
    assert!(err.to_string().contains("9"), "got: {err}");
}

#[test]
fn decode_keeps_high_score_boxes_across_strides() {
    let meta = ScaleMeta {
        scale: 0.5,
        pad_x: 10.0,
        pad_y: 20.0,
    };
    let dets = decode_outputs(
        &outputs(|i| if i == 0 || i == 4 { 0.9 } else { 0.1 }),
        0.5,
        0.4,
        &meta,
    )
    .unwrap();
    assert_eq!(dets.len(), 2, "两个 stride 各一个高分框应保留");
    assert!((dets[0].score - 0.9).abs() < f32::EPSILON);
    // padding 已逆映射回原图坐标（负值合法）。
    assert!(dets.iter().all(|d| d.landmarks_5pt.len() == 5));
}

#[test]
fn decode_skips_nan_score() {
    let meta = ScaleMeta {
        scale: 1.0,
        pad_x: 0.0,
        pad_y: 0.0,
    };
    let dets = decode_outputs(&outputs(|_| f32::NAN), 0.5, 0.4, &meta).unwrap();
    assert!(dets.is_empty(), "NaN 框应被显式判 NaN 后跳过");
}

#[test]
fn decode_skips_bbox_out_of_slice() {
    let meta = ScaleMeta {
        scale: 1.0,
        pad_x: 0.0,
        pad_y: 0.0,
    };
    let mut out = outputs(|i| if i == 0 { 0.9 } else { 0.0 });
    // bbox_8 缩短为 [1, 1, 3]：bo+4 > bbox.len() 触发 continue（含 idx 0 的 box）。
    out[1] = tvalue_of_f32(&[1, 1, 3], &[0.0; 3]);
    let dets = decode_outputs(&out, 0.5, 0.4, &meta).unwrap();
    assert!(dets.is_empty());
}

#[test]
fn decode_skips_kps_out_of_slice() {
    let meta = ScaleMeta {
        scale: 1.0,
        pad_x: 0.0,
        pad_y: 0.0,
    };
    let mut out = outputs(|i| if i == 0 { 0.9 } else { 0.0 });
    // kps_8 缩短为 [1, 1, 9]：ko+10 > kps.len() 触发 continue。
    out[2] = tvalue_of_f32(&[1, 1, 9], &[0.0; 9]);
    let dets = decode_outputs(&out, 0.5, 0.4, &meta).unwrap();
    assert!(dets.is_empty());
}

#[test]
fn nms_keeps_farther_box_and_drops_overlapping() {
    let mk = |cx: f32, scale: f32| FaceDetection {
        bbox: [cx - 2.0 * scale, -1.0, cx + 2.0 * scale, 1.0],
        score: 0.9,
        landmarks_5pt: [[0.0; 2]; 5],
    };
    // 两个几乎重合 → 与阈值 0.4 比较的 IoU > 0.4 → 只留 score 高的那个。
    let kept = nms(vec![mk(0.0, 1.0), mk(0.1, 1.0)], 0.4);
    assert_eq!(kept.len(), 1);
    // 相隔很远 → 都保留。
    let kept2 = nms(vec![mk(0.0, 1.0), mk(100.0, 1.0)], 0.4);
    assert_eq!(kept2.len(), 2);
}

#[test]
fn iou_handles_disjoint_and_zero_union() {
    // 不相交 → inter=0 → IoU=0。
    assert_eq!(iou(&[0.0, 0.0, 1.0, 1.0], &[5.0, 5.0, 6.0, 6.0]), 0.0);
    // 零面积 box（x2 <= x1）→ union=0 → 按约定返 0。
    assert_eq!(iou(&[0.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 0.0]), 0.0);
}

#[test]
fn scrfd_preprocess_builds_letterbox_tensor() {
    // 任意小图：scale > 1（放大小目标），返回 (NCHW tensor, ScaleMeta)。
    let bytes = image_for(48, 32);
    let (tensor, meta) = tidymedia::scrfd_preprocess(&bytes).unwrap();
    assert!(meta.scale > 1.0);
    assert!(meta.pad_x >= 0.0 && meta.pad_y >= 0.0);
    assert_eq!(tensor.shape().len(), 4);
}
