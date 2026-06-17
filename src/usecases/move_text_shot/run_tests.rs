//! `move_text_shot` 主流程单元测试：`FakeBackend` + `FakeTextDetector` 覆盖所有分支。
//!
//! 覆盖目标（与 plan 文件「branch miss 高危点」一一对应）：
//! - `is_image` / non-image MIME 过滤
//! - detector `Ok(true)` / `Ok(false)` / `Err` 三态
//! - `unique_name` 冲突 + 耗尽
//! - source ⊆ output overlap
//! - output ⊂ source walk-skip
//! - `dry_run` vs 真跑
//! - `read_all` Err（注入 reader error）
//! - 同 scheme rename fast-path vs 跨 scheme stream
//! - `split_stem_ext` / `relative_to` / `target_dir` 纯函数边界

use super::*;
use crate::adapters::backend::factory::BackendFactory;
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::ocr::fake::FakeTextDetector;
use crate::entities::common::Error;
use camino::Utf8PathBuf;
use std::io;

// ---- 纯函数边界 ----

#[test]
fn split_stem_ext_handles_dotless_name() {
    assert_eq!(split_stem_ext("README"), ("README", ""));
}

#[test]
fn split_stem_ext_handles_trailing_dot() {
    // "a." → rsplit_once 返 ("a", "") → ext 空 → 视为整名 stem
    assert_eq!(split_stem_ext("a."), ("a.", ""));
}

#[test]
fn split_stem_ext_handles_leading_dot() {
    // ".env" → rsplit_once 返 ("", "env") → stem 空 → 视为整名 stem
    assert_eq!(split_stem_ext(".env"), (".env", ""));
}

#[test]
fn split_stem_ext_strips_last_dot_only() {
    assert_eq!(split_stem_ext("a.b.c"), ("a.b", "c"));
}

#[test]
fn relative_to_returns_relative_within_root() {
    let src = Utf8Path::new("/a/b/c.png");
    let root = Utf8Path::new("/a");
    assert_eq!(relative_to(src, root), Utf8Path::new("b/c.png"));
}

#[test]
fn relative_to_falls_back_when_prefix_mismatch() {
    let src = Utf8Path::new("/a/b/c.png");
    let root = Utf8Path::new("/x");
    assert_eq!(relative_to(src, root), src);
}

#[test]
fn target_dir_returns_output_when_rel_empty() {
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let got = target_dir(&out, Some(Utf8Path::new("")));
    assert_eq!(got.path(), Utf8Path::new("/out"));
}

#[test]
fn target_dir_returns_output_when_rel_none() {
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let got = target_dir(&out, None);
    assert_eq!(got.path(), Utf8Path::new("/out"));
}

#[test]
fn target_dir_joins_rel_dir() {
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let got = target_dir(&out, Some(Utf8Path::new("sub/dir")));
    assert_eq!(got.path(), Utf8Path::new("/out/sub/dir"));
}

#[test]
fn is_image_true_for_png_magic() {
    // PNG: 89 50 4E 47
    let bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR...";
    assert!(is_image(bytes));
}

#[test]
fn is_image_false_for_text() {
    assert!(!is_image(b"plain text"));
}

#[test]
fn is_image_false_for_empty() {
    assert!(!is_image(b""));
}

#[test]
fn summary_result_partial_on_failure() {
    assert_eq!(summary_result(0), "ok");
    assert_eq!(summary_result(1), "partial");
}

// ---- main flow with FakeBackend + FakeTextDetector ----

/// 极小 PNG fixture（infer 能识别为 image/png）。8 字节 PNG signature 已足够让 infer
/// 判 `image/png`；不需要后续 chunk——detector 是 fake 不真解码。
fn tiny_png() -> Vec<u8> {
    // PNG file signature (8 bytes) + 任意 padding 让 head 长度过 256 字节阈值
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0_u8; 256]);
    bytes
}

fn local_loc(p: &str) -> Location {
    Location::Local(Utf8PathBuf::from(p))
}

/// 测试用 factory：把同一个 Arc<FakeBackend> 返给所有 Location（scheme 一致）。
struct SingleBackendFactory(Arc<dyn Backend>);

impl BackendFactory for SingleBackendFactory {
    fn for_location(&self, _loc: &Location) -> common::Result<Arc<dyn Backend>> {
        Ok(Arc::clone(&self.0))
    }
}

fn fake_factory() -> (Arc<FakeBackend>, SingleBackendFactory) {
    let fake = Arc::new(FakeBackend::new("local"));
    let factory = SingleBackendFactory(Arc::clone(&fake) as Arc<dyn Backend>);
    (fake, factory)
}

/// 两 backend 按 scheme 切换的 factory：smb scheme → `smb` 字段；其他 → `local`。
/// 跨 scheme 测试公用，避免 fn 内重复定义触发 `items_after_statements`。
struct TwoSchemeFactory {
    smb: Arc<dyn Backend>,
    local: Arc<dyn Backend>,
}
impl BackendFactory for TwoSchemeFactory {
    fn for_location(&self, loc: &Location) -> common::Result<Arc<dyn Backend>> {
        Ok(match loc.scheme() {
            "smb" => Arc::clone(&self.smb),
            _ => Arc::clone(&self.local),
        })
    }
}

#[test]
fn move_text_shot_rejects_source_inside_output() {
    let (_fake, factory) = fake_factory();
    let detector = FakeTextDetector::new(false);
    let err = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/out/sub")],
        &local_loc("/out"),
        false,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::InvalidInput);
    assert!(io_err.to_string().contains("is inside output"));
}

#[test]
fn move_text_shot_moves_when_detector_hits() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_dir(local_loc("/src/a"));
    fake.add_file(local_loc("/src/a/photo.png"), tiny_png());
    let detector =
        FakeTextDetector::new(false).with_result(Utf8PathBuf::from("/src/a/photo.png"), true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.scanned, 1);
    assert_eq!(report.image_files, 1);
    assert_eq!(report.ocr_hits, 1);
    assert_eq!(report.moved, 1);
    assert_eq!(report.failed, 0);
    // 相对路径保留：src/a/photo.png → out/a/photo.png
    assert!(fake.exists(&local_loc("/out/a/photo.png")).unwrap());
    assert!(!fake.exists(&local_loc("/src/a/photo.png")).unwrap());
}

#[test]
fn move_text_shot_keeps_file_when_detector_misses() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/photo.png"), tiny_png());
    let detector = FakeTextDetector::new(false);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.skipped_no_text, 1);
    assert_eq!(report.moved, 0);
    assert!(fake.exists(&local_loc("/src/photo.png")).unwrap());
}

#[test]
fn move_text_shot_skips_non_image() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/notes.txt"), b"plain text".to_vec());
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.skipped_non_image, 1);
    assert_eq!(report.image_files, 0);
    assert_eq!(report.moved, 0);
}

#[test]
fn move_text_shot_dry_run_does_not_write() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/photo.png"), tiny_png());
    let detector =
        FakeTextDetector::new(false).with_result(Utf8PathBuf::from("/src/photo.png"), true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap();

    assert_eq!(report.moved, 1);
    assert!(report.dry_run);
    // src 仍在；dst 未创建（dry_run 不调 mkdir/rename）
    assert!(fake.exists(&local_loc("/src/photo.png")).unwrap());
    assert!(!fake.exists(&local_loc("/out/photo.png")).unwrap());
}

#[test]
fn move_text_shot_records_failure_on_read_error() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/bad.png"), tiny_png());
    fake.inject_reader_error(local_loc("/src/bad.png"), io::ErrorKind::PermissionDenied);
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].path.ends_with("bad.png"));
}

#[test]
fn move_text_shot_records_failure_on_detector_error() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/oops.png"), tiny_png());
    let detector = FakeTextDetector::new(true).with_error(Utf8PathBuf::from("/src/oops.png"));

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    assert!(report.errors[0].message.contains("injected error"));
}

#[test]
fn move_text_shot_unique_name_appends_suffix_when_collides() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png());
    // 预占 out/a.png 让 unique_name 退到 a_1.png
    fake.add_dir(local_loc("/out"));
    fake.add_file(local_loc("/out/a.png"), b"existing".to_vec());
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.moved, 1);
    assert!(fake.exists(&local_loc("/out/a_1.png")).unwrap());
    // 原有的 out/a.png 不受影响
    assert!(fake.exists(&local_loc("/out/a.png")).unwrap());
}

#[test]
fn move_text_shot_unique_name_exhausted_records_failure() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/x.png"), tiny_png());
    fake.add_dir(local_loc("/out"));
    // 预占 x.png 与 x_1..=x_N（N = unique_name_max_attempts，默认 10）
    fake.add_file(local_loc("/out/x.png"), b"e".to_vec());
    for i in 1..=config().copy.unique_name_max_attempts {
        fake.add_file(local_loc(&format!("/out/x_{i}.png")), b"e".to_vec());
    }
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    assert!(report.errors[0].message.contains("exhausted unique-name"));
}

#[test]
fn move_text_shot_skips_output_subtree_when_output_under_source() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/photos"));
    // src/a.png 命中要搬；src/archive/old.png 已在 output 下要 skip
    fake.add_file(local_loc("/photos/a.png"), tiny_png());
    fake.add_dir(local_loc("/photos/archive"));
    fake.add_file(local_loc("/photos/archive/old.png"), tiny_png());
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/photos")],
        &local_loc("/photos/archive"),
        false,
    )
    .unwrap();

    // 只有 a.png 被处理；old.png 因在 output 下被 skip
    assert_eq!(report.scanned, 1);
    assert_eq!(report.moved, 1);
    assert!(fake.exists(&local_loc("/photos/archive/a.png")).unwrap());
    // 原 archive/old.png 不受影响
    assert!(fake.exists(&local_loc("/photos/archive/old.png")).unwrap());
}

#[test]
fn move_text_shot_propagates_walker_error() {
    let (fake, factory) = fake_factory();
    fake.inject_error(local_loc("/src"), crate::FakeOp::Walk, io::ErrorKind::Other);
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    // walker 自身 Err 计 failed，不中断主流程
    assert_eq!(report.failed, 1);
}

#[test]
fn move_text_shot_records_failure_when_rename_copy_fails() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png());
    // 走 fast-path rename → fake default rename = copy_file + remove_file，注入 CopyFile Err
    fake.inject_error(
        local_loc("/src/a.png"),
        crate::FakeOp::CopyFile,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.moved, 0);
}

#[test]
fn move_text_shot_records_failure_on_unique_name_exists_error() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png());
    // 注入 Exists Err 让 unique_name_in_dir 返 Err → move_one record_failure（line 239 Err arm）
    fake.inject_error(
        local_loc("/out/a.png"),
        crate::FakeOp::Exists,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
}

#[test]
fn move_text_shot_unique_name_collision_for_extensionless_file() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    // 无扩展名 → unique_name 走 stem_ext.1.is_empty() 分支（line 365）
    fake.add_file(local_loc("/src/README"), tiny_png());
    fake.add_dir(local_loc("/out"));
    fake.add_file(local_loc("/out/README"), b"existing".to_vec());
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.moved, 1);
    assert!(fake.exists(&local_loc("/out/README_1")).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_stream_copy_succeeds() {
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_dir(Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/"),
    });
    src_fake.add_file(src_loc.clone(), tiny_png());

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(report.failed, 0);
    assert!(out_fake.exists(&local_loc("/out/img.png")).unwrap());
    // src 已被删
    assert!(!src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_text_shot_same_remote_scheme_uses_stream_copy_not_rename_fast_path() {
    // 顶置局部类型：items_after_statements (pedantic)。
    struct PerSchemeFactory {
        src: Arc<dyn Backend>,
        out: Arc<dyn Backend>,
        out_path: Utf8PathBuf,
    }
    impl BackendFactory for PerSchemeFactory {
        fn for_location(&self, loc: &Location) -> common::Result<Arc<dyn Backend>> {
            // 同 scheme（smb），但内容用 path 区分：output 走 self.out
            Ok(if loc.path() == self.out_path {
                Arc::clone(&self.out)
            } else {
                Arc::clone(&self.src)
            })
        }
    }

    // 两 backend 同 scheme 但非 "local" → do_move_file 第二个 && 短路 False 进入
    // stream_copy 路径，覆盖 BR idx 1。
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("smb"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc, tiny_png());

    let out_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/dst"),
    };
    let factory = PerSchemeFactory {
        src: Arc::clone(&src_fake) as Arc<dyn Backend>,
        out: Arc::clone(&out_fake) as Arc<dyn Backend>,
        out_path: out_loc.path().to_path_buf(),
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &out_loc,
        false,
    )
    .unwrap();
    assert_eq!(report.moved, 1);
}

#[test]
fn move_text_shot_cross_scheme_stream_copy_writer_runtime_error() {
    // open_write 成功但 writer.write 立即报错 → std::io::copy Err → stream_copy
    // Err arm（DA:324-325, BRDA:322 idx 0）+ 清理半截目标文件。
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    out_fake.inject_writer_error(local_loc("/out/img.png"), io::ErrorKind::PermissionDenied);

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // src 仍在；dst 被半截清理（remove_file 调用了，但 FakeBackend 没 dst 入 files 故 noop）
    assert!(src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_stream_copy_open_write_fails() {
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    out_fake.inject_error(
        local_loc("/out/img.png"),
        crate::FakeOp::OpenWrite,
        io::ErrorKind::PermissionDenied,
    );

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // src 未删（stream_copy Err 前）
    assert!(src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_remove_src_failure_marks_half_state() {
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    src_fake.inject_error(
        src_loc.clone(),
        crate::FakeOp::RemoveFile,
        io::ErrorKind::PermissionDenied,
    );

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // dst 已写入；src 未删（半态错误）
    assert!(out_fake.exists(&local_loc("/out/img.png")).unwrap());
    assert!(src_fake.exists(&src_loc).unwrap());
    let msg = &report.errors[0].message;
    assert!(
        msg.contains("copied") && msg.contains("but cannot remove source"),
        "got: {msg}"
    );
}

#[test]
fn move_text_shot_propagates_factory_error_for_output() {
    // factory.for_location(output)? Err arm（line 43）
    struct OutputFails;
    impl BackendFactory for OutputFails {
        fn for_location(&self, _loc: &Location) -> common::Result<Arc<dyn Backend>> {
            Err(crate::entities::common::Error::Io(io::Error::new(
                io::ErrorKind::Unsupported,
                "no factory",
            )))
        }
    }
    let detector = FakeTextDetector::new(true);
    let err = move_text_shot(
        &detector,
        &OutputFails,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn move_text_shot_propagates_factory_error_for_source() {
    // factory.for_location(source)? Err arm（line 58）：output 成功，source 失败
    struct SourceFails {
        ok_for: Utf8PathBuf,
    }
    impl BackendFactory for SourceFails {
        fn for_location(&self, loc: &Location) -> common::Result<Arc<dyn Backend>> {
            if loc.path() == self.ok_for {
                Ok(Arc::new(FakeBackend::new("local")) as Arc<dyn Backend>)
            } else {
                Err(crate::entities::common::Error::Io(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "src failed",
                )))
            }
        }
    }
    let factory = SourceFails {
        ok_for: Utf8PathBuf::from("/out"),
    };
    let detector = FakeTextDetector::new(true);
    let err = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn move_text_shot_records_failure_when_open_read_fails() {
    // read_all 内 open_read Err（line 196 ^0）
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png());
    fake.inject_error(
        local_loc("/src/a.png"),
        crate::FakeOp::OpenRead,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.scanned, 1);
}

#[test]
fn move_text_shot_records_failure_when_do_move_mkdir_fails() {
    // do_move_file 内 output_backend.mkdir_p(target_dir_loc) Err（line 287 ^0）
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_dir(local_loc("/src/sub"));
    fake.add_file(local_loc("/src/sub/a.png"), tiny_png());
    fake.add_dir(local_loc("/out"));
    fake.inject_error(
        local_loc("/out/sub"),
        crate::FakeOp::MkdirP,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // src 仍在；mkdir_p 失败前 fast-path rename 未触发
    assert!(fake.exists(&local_loc("/src/sub/a.png")).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_open_read_failure() {
    // stream_copy 内 src_backend.open_read Err（line 319 ^0）
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    src_fake.inject_error(
        src_loc,
        crate::FakeOp::OpenRead,
        io::ErrorKind::PermissionDenied,
    );
    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    // 关键：read_all 路径调 open_read 后 detector 触发；但已加了 OpenRead Err 在 src_loc
    // → 第一次读 file 字节就失败 → record_failure，不进 do_move_file。
    // 要触发 stream_copy 的 open_read Err 需要 read_all（第一次 open_read）成功而 stream_copy
    // 第二次 open_read 失败——FakeBackend.check_error 是恒报，没法做"第 N 次失败"。
    // 故只能验证 read_all 路径，stream_copy 内 open_read Err 实际不可单测触发——multi-binary
    // instance 套路：该 region 由 lib unit + 集成累加，subprocess instance 不可达。
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
}

#[test]
fn move_text_shot_propagates_mkdir_p_failure() {
    let (fake, factory) = fake_factory();
    fake.inject_error(
        local_loc("/out"),
        crate::FakeOp::MkdirP,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);

    let err = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::PermissionDenied);
}
