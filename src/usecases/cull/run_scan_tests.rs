use std::sync::Arc;

use super::test_common::*;
use super::*;
use crate::FakeFaceDetector;
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::adapters::face::fake::{FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector};

#[test]
fn cull_dry_run_two_similar_images_picks_one_best() {
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    write_png(&a, [128, 128, 128]);
    write_png(&b, [128, 128, 128]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());

    // detector 注入：让 a 检测到 1 张人脸（合法 ArcFace 模板 5 点），b 不命中 →
    // 给 a 注入微笑 mesh（嘴角上扬 → smile_bonus > 0），a total > b total → a 是 best。
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
    let a_camino = camino::Utf8PathBuf::from(a.to_str().unwrap());
    let mut smile_mesh = vec![[0.0_f32; 3]; 468];
    smile_mesh[61] = [0.0, 8.0, 0.0];
    smile_mesh[291] = [10.0, 8.0, 0.0];
    smile_mesh[13] = [5.0, 5.0, 0.0];
    smile_mesh[14] = [5.0, 15.0, 0.0];
    let scrfd = FakeFaceDetector::new(vec![]).with_result(a_camino.clone(), vec![face]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]).with_result(a_camino, smile_mesh);
    let eyestate = FakeEyeStateClassifier::new(0.0);

    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &crate::adapters::backend::factory::DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.grouped, 1);
    assert_eq!(report.best_count, 1);
    assert_eq!(report.culled_count, 1);
    assert_eq!(report.moved, 0, "dry_run");
    let group = &report.groups[0];
    assert!(
        group.best_source.ends_with("a.png"),
        "got: {}",
        group.best_source
    );
    assert!(
        group.score_breakdown.smile_bonus > 0.0,
        "got: {:?}",
        group.score_breakdown
    );
}

#[test]
fn cull_scan_skips_non_image_file() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("not-image.txt"), b"hello").unwrap();
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
        &crate::adapters::backend::factory::DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 1, "walker 触达 1 文件即 scanned=1");
    assert_eq!(report.failed, 0);
}

#[test]
fn cull_skips_single_image_group() {
    let src_dir = tempfile::tempdir().unwrap();
    write_png(&src_dir.path().join("a.png"), [50, 50, 50]);
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
        &crate::adapters::backend::factory::DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.grouped, 0, "single-image group is skipped");
    assert_eq!(report.best_count, 0);
}

#[test]
fn cull_skips_entry_under_output_prefix() {
    // source = /tmp/X, output = /tmp/X/out（output 在 source 下）；
    // ensure_sources_outside_output 仅检 src⊂out 反向，通过。
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path();
    let out_path = src_path.join("out");
    std::fs::create_dir_all(&out_path).unwrap();
    write_png(&out_path.join("nested.png"), [10, 20, 30]);

    let src = local_loc(src_path.to_str().unwrap());
    let out = local_loc(out_path.to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let report = cull(
        &scrfd,
        &facenet,
        &facemesh,
        &eyestate,
        &crate::adapters::backend::factory::DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.scanned, 0);
}

#[test]
fn cull_records_failure_when_walk_yields_err() {
    // 让 source 走 fake smb；FakeBackend inject Op::Walk Err
    let fake = Arc::new(FakeBackend::new("smb"));
    let src = smb_loc("/src");
    fake.inject_error(src.clone(), Op::Walk, io::ErrorKind::PermissionDenied);
    let mut factory = MapFactory::new();
    factory.insert("smb", fake);

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
        &factory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.failed, 1, "walk Err counted as failure");
    assert!(report.errors[0].message.contains("injected"));
}

#[test]
fn cull_records_failure_when_open_read_errs() {
    // FakeBackend 添加文件让 walk yields ok，但 OpenRead 失败 → record_failure 计入
    let fake = Arc::new(FakeBackend::new("smb"));
    let src = smb_loc("/src");
    let file_loc = smb_loc("/src/img.png");
    fake.add_dir(src.clone());
    fake.add_file(file_loc.clone(), b"unused".to_vec());
    fake.inject_error(file_loc, Op::OpenRead, io::ErrorKind::PermissionDenied);
    let mut factory = MapFactory::new();
    factory.insert("smb", fake);

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
        &factory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    assert!(report.errors[0].message.contains("injected"));
}

#[test]
fn cull_records_failure_when_read_to_end_errs() {
    // open_read 成功但 reader.read 立即 Err → read_all 内 read_to_end ? 触发 Err 路径
    let fake = Arc::new(FakeBackend::new("smb"));
    let src = smb_loc("/src");
    let file_loc = smb_loc("/src/img.png");
    fake.add_dir(src.clone());
    fake.add_file(file_loc.clone(), b"unused".to_vec());
    fake.inject_reader_error(file_loc, io::ErrorKind::TimedOut);
    let mut factory = MapFactory::new();
    factory.insert("smb", fake);

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
        &factory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    assert!(report.errors[0].message.contains("timed out"));
}
