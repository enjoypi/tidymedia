use std::sync::Arc;

use super::test_common::*;
use super::*;
use crate::FakeFaceDetector;
use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::adapters::face::fake::{FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector};

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
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
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
fn cull_returns_err_when_factory_rejects_output() {
    // MapFactory 未注入 smb → output 是 smb URI 返 Unsupported
    let factory = MapFactory::new();
    let src_dir = tempfile::tempdir().unwrap();
    let src = local_loc(src_dir.path().to_str().unwrap());
    let out = smb_loc("/out");
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let err = cull(
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
    .unwrap_err();
    assert!(err.to_string().contains("no fake backend"), "got: {err}");
}

#[test]
fn cull_returns_err_when_factory_rejects_source() {
    // MapFactory 未注入 smb → source 是 smb URI 返 Unsupported
    let factory = MapFactory::new();
    let src = smb_loc("/src");
    let out_dir = tempfile::tempdir().unwrap();
    let out = local_loc(out_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let err = cull(
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
    .unwrap_err();
    assert!(err.to_string().contains("no fake backend"), "got: {err}");
}

#[test]
fn cull_mkdir_p_failure_propagates_when_not_dry_run() {
    // output 走 fake smb，inject MkdirP Err；source 走 local 空目录。
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    fake.inject_error(out.clone(), Op::MkdirP, io::ErrorKind::PermissionDenied);
    let mut factory = MapFactory::new();
    factory.insert("smb", fake);

    let src_dir = tempfile::tempdir().unwrap();
    let src = local_loc(src_dir.path().to_str().unwrap());
    let scrfd = FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let err = cull(
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
    .unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}
