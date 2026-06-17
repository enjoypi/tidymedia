//! `cull::run` 单元测试。Fake detectors 注入驱动整 pipeline。

use super::*;
use crate::FakeFaceDetector;
use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::face::fake::{FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector};
use camino::Utf8PathBuf;

fn local_loc(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn write_png(path: &std::path::Path, fill: [u8; 3]) {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let pixels = vec![fill[0]; 16 * 16 * 3];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 16, 16, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

#[test]
fn ensure_sources_outside_output_rejects_source_inside_output() {
    let src = local_loc("/x/y");
    let out = local_loc("/x");
    let err = ensure_sources_outside_output(&[src], &out.display()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("is inside output"), "got: {msg}");
}

#[test]
fn ensure_sources_outside_output_accepts_sibling() {
    let src = local_loc("/x/a");
    let out = local_loc("/x/b");
    ensure_sources_outside_output(&[src], &out.display()).unwrap();
}

#[test]
fn cull_empty_source_returns_ok_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let src = local_loc(tmp.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 512]);
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
    assert_eq!(report.scanned, 0);
    assert_eq!(report.grouped, 0);
    assert_eq!(report.failed, 0);
}

#[test]
fn cull_source_inside_output_returns_err() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let src = local_loc(sub.to_str().unwrap());
    let out = local_loc(tmp.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 512]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let err = cull(
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
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("is inside output"), "got: {msg}");
}

#[test]
fn cull_dry_run_two_similar_images_picks_one_best() {
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    // 两张同色 PNG → ahash 相同 → 同组
    write_png(&a, [128, 128, 128]);
    write_png(&b, [128, 128, 128]);
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());

    // detector 注入：让 a 检测到 1 张人脸，b 检测到 0 张 → a 应为 best
    let face = crate::FaceDetection {
        bbox: [0.0, 0.0, 10.0, 10.0],
        score: 0.9,
        landmarks_5pt: [[1.0; 2]; 5],
    };
    let a_camino = camino::Utf8PathBuf::from(a.to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]).with_result(a_camino, vec![face]);
    let facenet = FakeFaceEmbedder::new([0.0; 512]);
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
}

#[test]
fn cull_scan_skips_non_image_file() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("not-image.txt"), b"hello").unwrap();
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 512]);
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
    assert_eq!(report.scanned, 0, "non-image skipped before scanned++");
    assert_eq!(report.failed, 0);
}

#[test]
fn cull_records_failure_on_corrupt_image() {
    let src_dir = tempfile::tempdir().unwrap();
    // PNG magic + 垃圾内容 → infer 识别为 image/png 但 image::load_from_memory 失败
    std::fs::write(
        src_dir.path().join("bad.png"),
        [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
            0xFF, 0xFF, 0xFF, 0xFF, // 垃圾
        ],
    )
    .unwrap();
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 512]);
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
