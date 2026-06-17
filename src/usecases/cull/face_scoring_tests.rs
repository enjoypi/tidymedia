//! `face_scoring` 单测：覆盖无脸 / 单脸张眼 / EAR 闭眼 / `EyeState` 闭眼 / 双印证 /
//! mesh 退化 / 微笑非负 / weight=0 / 嘴宽退化。

use super::*;

fn default_cfg() -> FaceConfig {
    FaceConfig::default()
}

fn sample_detection() -> FaceDetection {
    FaceDetection {
        bbox: [0.0, 0.0, 100.0, 100.0],
        score: 0.99,
        landmarks_5pt: [[10.0; 2]; 5],
    }
}

/// 构造 468 点 mesh，所有点初始 (10, 10, 0)；用 closure 覆盖关键 6 点+4 嘴点。
fn make_mesh(eye_ratio: f32, smile_curl: f32) -> Vec<[f32; 3]> {
    let mut mesh = vec![[10.0_f32; 3]; MESH_POINT_COUNT];
    // 左眼 6 点：p1 外角 (0, 5), p4 内角 (10, 5) → horiz = 10
    //          p2/p6 上/下 (3, 5 ± 5*ratio/2)
    //          p3/p5 上/下 (7, 5 ± 5*ratio/2)
    //   EAR = (vert1 + vert2) / (2 * horiz) = (5*ratio + 5*ratio) / 20 = ratio/2
    let half_v = 5.0 * eye_ratio / 2.0;
    set_eye(&mut mesh, LEFT_EYE_IDX, half_v);
    set_eye(&mut mesh, RIGHT_EYE_IDX, half_v);
    // 嘴：左角 (0, 10 + curl), 右角 (10, 10 + curl), 上唇 (5, 5), 下唇 (5, 15)
    //   center_y = 10, mouth_width = 10
    //   curl_avg = curl，smile = -curl / 10 ∈ [-, +]
    mesh[MOUTH_LEFT] = [0.0, 10.0 + smile_curl, 0.0];
    mesh[MOUTH_RIGHT] = [10.0, 10.0 + smile_curl, 0.0];
    mesh[LIP_UPPER] = [5.0, 5.0, 0.0];
    mesh[LIP_LOWER] = [5.0, 15.0, 0.0];
    mesh
}

fn set_eye(mesh: &mut [[f32; 3]], idx: [usize; 6], half_v: f32) {
    mesh[idx[0]] = [0.0, 5.0, 0.0]; // 外角
    mesh[idx[3]] = [10.0, 5.0, 0.0]; // 内角
    mesh[idx[1]] = [3.0, 5.0 - half_v, 0.0]; // 上 p2
    mesh[idx[5]] = [3.0, 5.0 + half_v, 0.0]; // 下 p6
    mesh[idx[2]] = [7.0, 5.0 - half_v, 0.0]; // 上 p3
    mesh[idx[4]] = [7.0, 5.0 + half_v, 0.0]; // 下 p5
}

#[test]
fn score_image_no_faces_returns_pure_sharpness() {
    let cfg = default_cfg();
    let s = score_image(50.0, &[], &[], &[], &cfg);
    assert!((s.sharpness - 50.0).abs() < f32::EPSILON);
    assert!(s.blink_penalty.abs() < f32::EPSILON);
    assert!(s.smile_bonus.abs() < f32::EPSILON);
    assert!((s.total - 50.0 * cfg.w_sharpness).abs() < f32::EPSILON);
}

#[test]
fn score_image_open_eyes_no_smile_no_penalty() {
    // EAR ratio=0.5 → EAR=0.25 > ear_blink_max=0.21；EyeState 概率 0.1 < 0.5
    let cfg = default_cfg();
    let mesh = make_mesh(0.5, 0.0);
    let s = score_image(
        100.0,
        &[sample_detection()],
        &[mesh],
        &[(0.1_f32, 0.1)],
        &cfg,
    );
    assert!(s.blink_penalty.abs() < f32::EPSILON, "got: {s:?}");
}

#[test]
fn score_image_ear_below_threshold_counts_blink() {
    // ratio=0.2 → EAR=0.1 < 0.21 → EAR 命中闭眼
    let cfg = default_cfg();
    let mesh = make_mesh(0.2, 0.0);
    let s = score_image(0.0, &[sample_detection()], &[mesh], &[(0.1_f32, 0.1)], &cfg);
    assert!((s.blink_penalty - cfg.w_blink).abs() < 1e-5, "got: {s:?}");
}

#[test]
fn score_image_eyestate_above_threshold_counts_blink() {
    // EAR ratio=0.5 → 不命中；EyeState 0.9 > 0.5 → 命中
    let cfg = default_cfg();
    let mesh = make_mesh(0.5, 0.0);
    let s = score_image(0.0, &[sample_detection()], &[mesh], &[(0.9_f32, 0.1)], &cfg);
    assert!((s.blink_penalty - cfg.w_blink).abs() < 1e-5);
}

#[test]
fn score_image_double_match_counts_blink_once() {
    // EAR 命中 + EyeState 命中 → 仍 1 次惩罚（不双计）
    let cfg = default_cfg();
    let mesh = make_mesh(0.2, 0.0);
    let s = score_image(0.0, &[sample_detection()], &[mesh], &[(0.9_f32, 0.9)], &cfg);
    assert!((s.blink_penalty - cfg.w_blink).abs() < 1e-5);
}

#[test]
fn score_image_mesh_under_size_skips_ear_judgment() {
    // mesh 仅 10 点 < 468 → EAR Some(None) → 不命中；EyeState 还是判
    let cfg = default_cfg();
    let s = score_image(
        0.0,
        &[sample_detection()],
        &[vec![[0.0_f32; 3]; 10]],
        &[(0.9_f32, 0.0)],
        &cfg,
    );
    assert!((s.blink_penalty - cfg.w_blink).abs() < 1e-5, "got: {s:?}");
}

#[test]
fn score_image_smile_curl_positive_yields_bonus() {
    // curl 负值 → 嘴角 y < center → 上扬 → smile_bonus > 0
    let cfg = default_cfg();
    let mesh = make_mesh(0.5, -2.0); // 嘴角 y=8 < center 10
    let s = score_image(0.0, &[sample_detection()], &[mesh], &[(0.0_f32, 0.0)], &cfg);
    // smile = -(-2)/10 = 0.2，bonus = w_smile * 0.2 = 0.5 * 0.2 = 0.1
    assert!((s.smile_bonus - 0.1).abs() < 1e-3, "got: {s:?}");
}

#[test]
fn score_image_smile_curl_negative_clamps_to_zero() {
    // curl > 0 → 嘴角下垂 → smile.max(0) = 0
    let cfg = default_cfg();
    let mesh = make_mesh(0.5, 2.0);
    let s = score_image(0.0, &[sample_detection()], &[mesh], &[(0.0_f32, 0.0)], &cfg);
    assert!(s.smile_bonus.abs() < 1e-5, "got: {s:?}");
}

#[test]
fn score_image_all_weights_zero_yields_total_zero() {
    let mut cfg = default_cfg();
    cfg.w_sharpness = 0.0;
    cfg.w_blink = 0.0;
    cfg.w_smile = 0.0;
    let mesh = make_mesh(0.2, -2.0);
    let s = score_image(
        100.0,
        &[sample_detection()],
        &[mesh],
        &[(0.9_f32, 0.9)],
        &cfg,
    );
    assert!(s.total.abs() < 1e-5);
}

#[test]
fn score_image_missing_eye_state_skips_eyestate_judgment() {
    // 2 张脸但 eye_states.len() = 1 → 第二张不查 EyeState（只 EAR）
    let cfg = default_cfg();
    let m_open = make_mesh(0.5, 0.0);
    let m_closed = make_mesh(0.2, 0.0);
    let s = score_image(
        0.0,
        &[sample_detection(), sample_detection()],
        &[m_open, m_closed],
        &[(0.0_f32, 0.0)], // 仅 1 个，覆盖第一张脸
        &cfg,
    );
    // 第一张：EAR=0.25 > 0.21 + EyeState=0 → 不闭；
    // 第二张：EAR=0.1 < 0.21 + 无 EyeState → 仍闭（EAR 单独命中）。
    assert!((s.blink_penalty - cfg.w_blink).abs() < 1e-5);
}

#[test]
fn ear_at_indices_zero_horizontal_returns_none() {
    let mut mesh = vec![[10.0_f32; 3]; MESH_POINT_COUNT];
    // 外角 = 内角 → horiz = 0
    mesh[LEFT_EYE_IDX[0]] = [5.0, 5.0, 0.0];
    mesh[LEFT_EYE_IDX[3]] = [5.0, 5.0, 0.0];
    let ear = ear_at_indices(&mesh, LEFT_EYE_IDX);
    assert!(ear.is_none());
}

#[test]
fn ear_from_mesh_right_eye_degenerate_returns_none() {
    // 左眼 6 点合法（ratio=0.5 → EAR=0.25），右眼外/内角同点 → 右眼 ear_at_indices None
    // → ear_from_mesh 在右眼 `?` Err arm 早返 None。
    let mut mesh = vec![[10.0_f32; 3]; MESH_POINT_COUNT];
    set_eye(&mut mesh, LEFT_EYE_IDX, 1.25);
    mesh[RIGHT_EYE_IDX[0]] = [3.0, 5.0, 0.0];
    mesh[RIGHT_EYE_IDX[3]] = [3.0, 5.0, 0.0];
    assert!(ear_from_mesh(&mesh).is_none());
}

#[test]
fn ear_from_mesh_under_size_returns_none() {
    let mesh = vec![[0.0_f32; 3]; 10];
    assert!(ear_from_mesh(&mesh).is_none());
}

#[test]
fn smile_from_mesh_under_size_returns_none() {
    let mesh = vec![[0.0_f32; 3]; 10];
    assert!(smile_from_mesh(&mesh).is_none());
}

#[test]
fn smile_from_mesh_zero_mouth_width_returns_zero() {
    let mut mesh = vec![[10.0_f32; 3]; MESH_POINT_COUNT];
    mesh[MOUTH_LEFT] = [5.0, 10.0, 0.0];
    mesh[MOUTH_RIGHT] = [5.0, 10.0, 0.0]; // 左右嘴角同 x → mouth_width = 0
    mesh[LIP_UPPER] = [5.0, 8.0, 0.0];
    mesh[LIP_LOWER] = [5.0, 12.0, 0.0];
    assert_eq!(smile_from_mesh(&mesh), Some(0.0));
}

#[test]
fn dist_2d_pythagoras() {
    let d = dist_2d([0.0, 0.0], [3.0, 4.0]);
    assert!((d - 5.0).abs() < 1e-5);
}
