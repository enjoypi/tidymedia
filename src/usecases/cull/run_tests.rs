//! `cull::run` 单元测试。Fake detectors 注入驱动整 pipeline。

use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::FakeFaceDetector;
use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::adapters::backend::local::LocalBackend;
use crate::adapters::face::fake::{FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector};
use crate::entities::backend::Backend;
use camino::Utf8PathBuf;

fn local_loc(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "x".into(),
        path: Utf8PathBuf::from(path),
    }
}

/// 内部测试用 factory：local 走真实 `LocalBackend`，其他 scheme 从 map 取。
struct MapFactory {
    by_scheme: HashMap<&'static str, Arc<dyn Backend>>,
}

impl MapFactory {
    fn new() -> Self {
        Self {
            by_scheme: HashMap::new(),
        }
    }
    fn insert(&mut self, scheme: &'static str, b: Arc<dyn Backend>) {
        self.by_scheme.insert(scheme, b);
    }
}

impl crate::adapters::backend::factory::BackendFactory for MapFactory {
    fn for_location(&self, loc: &Location) -> crate::entities::common::Result<Arc<dyn Backend>> {
        if let Some(b) = self.by_scheme.get(loc.scheme()) {
            return Ok(Arc::clone(b));
        }
        if matches!(loc, Location::Local(_)) {
            return Ok(LocalBackend::arc());
        }
        Err(crate::entities::common::Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("no fake backend for scheme {}", loc.scheme()),
        )))
    }
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
fn cull_dry_run_two_similar_images_picks_one_best() {
    let src_dir = tempfile::tempdir().unwrap();
    let a = src_dir.path().join("a.png");
    let b = src_dir.path().join("b.png");
    // 两张同色 PNG → phash 相同 → 同组
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
    // MediaPipe 4 嘴部索引：61 / 291 / 13 / 14
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
fn cull_returns_err_when_factory_rejects_output() {
    // MapFactory 未注 smb → output 是 smb URI 返 Unsupported
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
    // MapFactory 不含 mtp → source 是 mtp URI 返 Unsupported
    let factory = MapFactory::new();
    let src = Location::Mtp {
        device: "x".into(),
        storage: "y".into(),
        path: camino::Utf8PathBuf::from("/"),
    };
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
        false, // 非 dry-run → 触发 mkdir_p
        10,
    )
    .unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn cull_skips_single_image_group() {
    // 单张图 → ahash 自成一组（len=1）→ line 107 continue
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
        &DefaultBackendFactory,
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
fn cull_skips_entry_under_output_prefix() {
    // source = /tmp/X, output = /tmp/X/out（output 在 source 下）；
    // ensure_sources_outside_output 仅检 src⊂out 反向，通过。
    // walk source 应跳过 output 子目录内的文件，命中 under_prefix continue。
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path();
    let out_path = src_path.join("out");
    std::fs::create_dir_all(&out_path).unwrap();
    // 在 output 下放一张 PNG，walk source 会枚举到它，应跳过
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
        &DefaultBackendFactory,
        &[src],
        &out,
        true,
        10,
    )
    .unwrap();
    // nested.png 在 output prefix 下被过滤
    assert_eq!(report.scanned, 0);
}

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
    // 仍选出 best（用兜底逻辑）+ 1 culled
    assert_eq!(report.best_count, 1);
    assert_eq!(report.culled_count, 1);
}

#[test]
fn cull_records_failure_when_oversize_image_skipped() {
    // 切独立 config 让 max_image_bytes = 1048576（1 MiB；正好等于 sanitize 下限不被回退）。
    // 写一张 1500×1500 不可压缩 random-noise PNG（>1 MiB）→ scan_source 内 size 超阈值
    // → record_failure（不读字节即跳过），failed=1，scanned=0。
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        "backend:\n  face:\n    max_image_bytes: 1048576\n",
    )
    .unwrap();
    // SAFETY: nextest per-test 进程隔离，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }

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
    assert_eq!(report.scanned, 0, "超 1 MiB PNG 被 OOM skip");
    assert_eq!(report.failed, 1);
    assert!(
        report.errors[0].message.contains("max_image_bytes"),
        "got: {}",
        report.errors[0].message
    );
}

/// 写一张 `side×side` random-noise PNG 让 PNG 压缩不下来，保证字节数 > 1 MiB。
fn write_random_png(path: &std::path::Path, side: u32) {
    use image::ImageEncoder;
    let total = (side as usize) * (side as usize) * 3;
    let mut pixels = vec![0_u8; total];
    for (i, p) in pixels.iter_mut().enumerate() {
        // 高熵 noise 模式（不可压缩）：每像素 ((i * 37) ^ (i >> 3)) mod 256
        *p = u8::try_from((i.wrapping_mul(37) ^ (i >> 3)) & 0xff).expect("internal: & 0xff < 256");
    }
    let mut out = Vec::with_capacity(total);
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, side, side, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

#[test]
fn cull_records_failure_when_write_group_errs() {
    // 非 dry-run：两张同 ahash PNG 进同组，让 group_writer mkdir_p group 子目录失败。
    // output 走 fake smb，inject MkdirP Err on output（首 mkdir_p 已通过，第二
    // mkdir_p 在 group_writer 内调用同 backend）—— FakeBackend 错误是按 (loc, op) 匹配，
    // 全 smb output 路径都拒。改成只 inject group 子目录路径的 MkdirP Err。
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    // group_writer 算的 group dir = /out/group-001 （source root = src_dir, best 顶层）
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
        false, // 非 dry-run 触发 group_writer
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
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
            0xFF, 0xFF, 0xFF, 0xFF, // 垃圾
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
