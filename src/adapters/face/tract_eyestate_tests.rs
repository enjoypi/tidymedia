//! `tract_eyestate` 单元测试 — `YOLOv8` 检测头版。

use super::*;
use camino::Utf8Path;

/// `dummy` output shape `[1, 6, anchors]`，按 channel-major flat layout（与 tract
/// `as_slice` 顺序一致）。`closed_max` 注入 closed channel 的最大 conf。
fn build_yolo_output(anchors: usize, closed_max: f32) -> Tensor {
    let mut data = vec![0.0_f32; OUTPUT_CHANNELS * anchors];
    // closed channel (index 5) 的最后一个 anchor 注入 max
    let closed_offset = (BOX_DIMS + CLOSED_CLASS_IDX) * anchors;
    if anchors > 0 {
        data[closed_offset + anchors - 1] = closed_max;
    }
    tract_ndarray::Array3::from_shape_vec((1, OUTPUT_CHANNELS, anchors), data)
        .expect("stub yolo output shape")
        .into_tensor()
}

struct ConstRaw {
    closed_max: f32,
    anchors: usize,
}

impl RawEyeState for ConstRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Ok(build_yolo_output(self.anchors, self.closed_max))
    }
}

struct FailRaw;
impl RawEyeState for FailRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        Err(io::Error::other("stub eyestate failed"))
    }
}

fn cfg() -> FaceConfig {
    FaceConfig {
        eyestate_model_path: "ignored-by-stub".into(),
        ..FaceConfig::default()
    }
}

fn tiny_eye() -> image::RgbImage {
    image::RgbImage::from_pixel(8, 8, image::Rgb([50, 50, 50]))
}

#[test]
fn build_eyestate_classifier_rejects_empty_path() {
    let e = build_eyestate_classifier(&FaceConfig::default()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(
        e.to_string().contains("eyestate_model_path is empty"),
        "got: {e}"
    );
}

#[test]
fn build_eyestate_classifier_rejects_whitespace_only_path() {
    let c = FaceConfig {
        eyestate_model_path: "   ".into(),
        ..FaceConfig::default()
    };
    let e = build_eyestate_classifier(&c).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn classify_eye_returns_high_when_closed_conf_dominates() {
    let det = TractEyeStateClassifier::with_raw(
        cfg(),
        Box::new(ConstRaw {
            closed_max: 0.9,
            anchors: 8400,
        }),
    );
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!((p - 0.9).abs() < 1e-5, "got: {p}");
}

#[test]
fn classify_eye_returns_low_when_all_anchors_silent() {
    let det = TractEyeStateClassifier::with_raw(
        cfg(),
        Box::new(ConstRaw {
            closed_max: 0.0,
            anchors: 8400,
        }),
    );
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!(p.abs() < f32::EPSILON, "got: {p}");
}

#[test]
fn classify_eye_clamps_conf_above_one() {
    // 防御性：模型若输出 >1.0 的原始 logit-without-sigmoid，clamp 到 [0,1]
    let det = TractEyeStateClassifier::with_raw(
        cfg(),
        Box::new(ConstRaw {
            closed_max: 5.0,
            anchors: 100,
        }),
    );
    let p = det.classify_eye(Utf8Path::new("/x"), &tiny_eye()).unwrap();
    assert!((p - 1.0).abs() < f32::EPSILON, "got: {p}");
}

#[test]
fn classify_eye_propagates_raw_error() {
    let det = TractEyeStateClassifier::with_raw(cfg(), Box::new(FailRaw));
    let e = det
        .classify_eye(Utf8Path::new("/x"), &tiny_eye())
        .unwrap_err();
    assert!(e.to_string().contains("stub eyestate failed"), "got: {e}");
}

#[test]
fn decode_rejects_wrong_rank() {
    let t = tract_ndarray::Array2::from_shape_vec((1, 6), vec![0.0_f32; 6])
        .unwrap()
        .into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(e.to_string().contains("!= [1, 6, anchors]"), "got: {e}");
}

#[test]
fn decode_rejects_wrong_channel_count() {
    let t = tract_ndarray::Array3::from_shape_vec((1, 4, 8400), vec![0.0_f32; 4 * 8400])
        .unwrap()
        .into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(e.to_string().contains("!= [1, 6, anchors]"), "got: {e}");
}

#[test]
fn decode_rejects_zero_anchors() {
    let t = tract_ndarray::Array3::from_shape_vec((1, 6, 0), Vec::<f32>::new())
        .unwrap()
        .into_tensor();
    let e = decode(&t).unwrap_err();
    assert!(e.to_string().contains("0 anchors"), "got: {e}");
}

#[test]
fn preprocess_letterbox_square_input() {
    // 正方形输入 → 无 padding，整图占满 640×640
    let exact = image::RgbImage::from_pixel(320, 320, image::Rgb([100, 100, 100]));
    let t = preprocess(&exact).unwrap();
    assert_eq!(t.shape(), [1, 3, 640, 640]);
}

#[test]
fn preprocess_letterbox_landscape_pads_vertical() {
    // 横向 640×320 → resize 长边 640 后 height=320，上下各 padding 160 行 114/255
    let wide = image::RgbImage::from_pixel(640, 320, image::Rgb([200, 100, 50]));
    let t = preprocess(&wide).unwrap();
    assert_eq!(t.shape(), [1, 3, 640, 640]);
}

#[test]
fn preprocess_handles_zero_dim_input() {
    let empty = image::RgbImage::new(0, 10);
    let t = preprocess(&empty).unwrap();
    assert_eq!(t.shape(), [1, 3, 640, 640]);
}

#[test]
fn detector_debug_includes_loaded_state() {
    let det = TractEyeStateClassifier::with_raw(
        cfg(),
        Box::new(ConstRaw {
            closed_max: 0.0,
            anchors: 100,
        }),
    );
    let s = format!("{det:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}
