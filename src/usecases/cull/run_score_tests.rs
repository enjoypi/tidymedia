use std::sync::Arc;

use super::test_common::*;
use super::*;
use crate::FakeFaceDetector;
use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::backend::fake::Op;
use crate::adapters::face::fake::{FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector};

#[test]
fn cull_records_failure_when_scrfd_detect_errs_on_all() {
    // 两张同色 PNG → 同组 → analyze_image 调 SCRFD 时全部返 Err → 记 failure 2 次，
    // best_total 仍 NEG_INFINITY 走兜底用 indices[0] 的 sharpness 作 total。
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    write_png(&a, [60, 60, 60]);
    write_png(&b, [60, 60, 60]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![])
        .with_error(camino::Utf8PathBuf::from(a.to_str().unwrap()))
        .with_error(camino::Utf8PathBuf::from(b.to_str().unwrap()));
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.failed, 2, "两张 SCRFD Err 都记 failed");
    assert_eq!(report.best_count, 1);
    assert_eq!(report.culled_count, 1);
}

#[test]
fn cull_records_failure_when_oversize_image_skipped() {
    // 切独立 config 让 max_image_bytes = 1048576（1 MiB；正好等于 sanitize 下限不被回退）。
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        "backend:\n  face:\n    max_image_bytes: 1048576\n",
    )
    .unwrap();
    crate::reset_config_loader();
    // SAFETY: nextest per-test 进程隔离，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    crate::install_config_loader();

    let src_dir = tempfile::tempdir().unwrap();
    let big_path = src_dir.path().join("big.png");
    write_random_png(&big_path, 1500);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 1, "超阈值文件仍计入 walker 触达 scanned");
    assert_eq!(report.failed, 1);
    assert!(
        report.errors[0].message.contains("max_image_bytes"),
        "got: {}",
        report.errors[0].message
    );
}

#[test]
fn cull_records_failure_when_write_group_errs() {
    // 非 dry-run：两张同 ahash PNG 进同组，让 group_writer mkdir_p group 子目录失败。
    // 只 inject group 子目录路径的 MkdirP Err（首 mkdir_p 在 output 根，不注入）。
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let group_dir = smb_loc("/out/group-001");
    fake.inject_error(group_dir, Op::MkdirP, io::ErrorKind::PermissionDenied);
    let mut factory = MapFactory::new();
    factory.insert("smb", fake);

    let src_dir = tempfile::tempdir().unwrap();
    write_png(&src_dir.path().join("a.png"), [200, 200, 200]);
    write_png(&src_dir.path().join("b.png"), [200, 200, 200]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &factory,
        &[src],
        &out,
        false,
        10,
    )
    .unwrap();
    assert!(report.failed >= 1, "write_group 失败计入 failed");
}

#[test]
fn cull_records_failure_on_corrupt_image() {
    let src_dir = tempfile::tempdir().unwrap();
    // PNG magic + 垃圾内容 → infer 识别为 image/png 但 image::load_from_memory 失败
    std::fs::write(
        src_dir.path().join("bad.png"),
        [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0xFF, 0xFF, 0xFF,
        ],
    )
    .unwrap();
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.errors.len(), 1);
}

#[test]
fn cull_skips_face_when_align_fit_is_singular() {
    // SCRFD 返 5 点重合的 face → fit_similarity 矩阵奇异 → align_face Err → continue
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    write_png(&a, [70, 70, 70]);
    write_png(&b, [70, 70, 70]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let singular = crate::FaceDetection {
        bbox: [0.0, 0.0, 16.0, 16.0],
        score: 0.9,
        landmarks_5pt: [[1.0_f32; 2]; 5],
    };
    let scrfd = FakeFaceDetector::new(vec![])
        .with_result(
            camino::Utf8PathBuf::from(a.to_str().unwrap()),
            vec![singular],
        )
        .with_result(
            camino::Utf8PathBuf::from(b.to_str().unwrap()),
            vec![singular],
        );
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.best_count, 1);
}

#[test]
fn cull_skips_face_when_embedder_returns_err() {
    // SCRFD valid 5 点 + facenet.with_error → align ok → embed_face Err → continue
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    write_png(&a, [110, 110, 110]);
    write_png(&b, [110, 110, 110]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let face = crate::FaceDetection {
        bbox: [0.0, 0.0, 16.0, 16.0],
        score: 0.9,
        landmarks_5pt: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
    };
    let a_path = camino::Utf8PathBuf::from(a.to_str().unwrap());
    let b_path = camino::Utf8PathBuf::from(b.to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![])
        .with_result(a_path.clone(), vec![face])
        .with_result(b_path.clone(), vec![face]);
    let facenet = FakeFaceEmbedder::new([0.0; 128])
        .with_error(a_path)
        .with_error(b_path);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.best_count, 1);
}
