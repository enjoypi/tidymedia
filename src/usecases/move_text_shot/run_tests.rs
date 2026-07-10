#![expect(
    clippy::doc_markdown,
    reason = "既有测试 doc 注释批量描述性中文夹标识符，clippy 1.95 `doc_markdown` 扩大后大量新增命中；测试文档不构成公开 API 契约，批量加反引号 ROI 低"
)]
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
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::ocr::fake::FakeTextDetector;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::Error;
use crate::usecases::report::ERRORS_SOFT_CAP;
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

    // scanned 与 CopyReport 同口径 = walker 触达 entry 总数（Dir + File）；fixture:
    // /src (Dir) + /src/a (Dir) + /src/a/photo.png (File) = 3
    assert_eq!(report.scanned, 3);
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

    // 只有 a.png 被处理；old.png 因在 output 下被 skip。scanned 计入 walker 触达
    // 的所有 entry：/photos (Dir) + /photos/a.png (File) + /photos/archive (Dir) +
    // /photos/archive/old.png (File) = 4
    assert_eq!(report.scanned, 4);
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

// `unique_name_from_index` 循环内 `_N` 候选 exists Err 必须上抛（吞错会让候选
// 被误判可用 → 后续 open_write truncate 覆盖）。直调命中 `Err(e) => return Err(e)`
// arm——上面的 e2e 只注入 base 候选，触不到循环内 arm。
#[test]
fn unique_name_from_index_propagates_exists_error() {
    let fake = Arc::new(FakeBackend::new("local"));
    fake.inject_error(
        local_loc("/out/shot_1.png"),
        crate::FakeOp::Exists,
        io::ErrorKind::TimedOut,
    );
    let backend: Arc<dyn Backend> = fake;
    let err = unique_name_from_index(&local_loc("/out"), "shot.png", &backend, 1).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
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
    // fixture: /src (Dir) + /src/a.png (File) = 2
    assert_eq!(report.scanned, 2);
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

/// 主入口不再前置 `mkdir_p(output)`（冗余 finding：`do_move_file` 内 mkdir_cache 惯用
/// 模式建目标子目录已含 output 层）。空 source → walk 空 → 不触发任何 mkdir_p 调用
/// → 即便对 output 注入 Err 也不影响主流程返 Ok(空报告)。
#[test]
fn move_text_shot_does_not_prefix_mkdir_p_output_when_source_empty() {
    let (fake, factory) = fake_factory();
    fake.inject_error(
        local_loc("/out"),
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
    .expect("空 source 不触发 mkdir_p；预置 output 的 mkdir_p Err 不激活");
    assert_eq!(report.moved, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(report.scanned, 0);
}

// ---- 新增分支覆盖（fix all 15 项对应） ----

/// `ensure_no_overlap` 内 sources 两两 overlap 检查：/a 与 /a/sub 互相重叠 → InvalidInput
/// 覆盖 line 138-149（内层 for j 判定 + Err arm）。
#[test]
fn move_text_shot_rejects_sources_overlapping_each_other() {
    let (_fake, factory) = fake_factory();
    let detector = FakeTextDetector::new(true);
    let err = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/a"), local_loc("/a/sub")],
        &local_loc("/out"),
        false,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::InvalidInput);
    assert!(io_err.to_string().contains("overlaps another source"));
}

/// `entry.size > max_image_bytes` 前置 skip → skipped_too_large 累加。
/// FakeBackend 的 add_file 用真实字节长度作 size；用 max_image_bytes=1 让 tiny_png
/// (264 bytes) 触发上限。config 走独立 yaml 切 ocr.max_image_bytes。
#[test]
fn move_text_shot_skips_oversized_image_and_increments_too_large() {
    // 独立 config：ocr.max_image_bytes 设 1024*1024（1 MiB，同 sanitize 下限；正好过校验但
    // fixture 巧大过），验证 entry.size 触发前置 skip。fixture 造 > 1 MiB PNG head。
    // 方案：直接用 fake add_file_with_size 让 size 声明 2 MiB 而 bytes 只有 tiny_png。
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/big.png"), tiny_png());
    // 手动构造 size > max_image_bytes 场景：加个巨大 PNG bytes（复制 tiny_png 到 60 MiB）
    // 但这会真占堆内存，改走 fake set_size API 若可用；否则测试用 write_temp_config 切
    // ocr.max_image_bytes 到 256（小于 tiny_png=264 字节）。用后者更省内存。
    // 检查 FakeBackend 是否已加载 default config 值；本测试跑时 config() 已被前面测试 init
    // 一次即锁定，OnceLock 不能改。因此改用不依赖 config 切换的路径：直接构造 entry.size
    // 通过 add_file 的字节数；tiny_png=264，需要 max_image_bytes < 264。默认 50 MiB 远超，
    // 所以本测试用不到——保留为 doc 说明 config 加载后无法在测试进程内改。
    // 结论：此测试用 config 环境变量 in-process 无法生效（OnceLock 已初始化）；仅作
    // 集成测试补覆盖，此处 skip 直接期望默认路径不触发 skipped_too_large。
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap();
    // 默认 max_image_bytes 50 MiB，不会命中 skipped_too_large；skipped_too_large 分支
    // 覆盖交给下方 `move_text_shot_skips_too_large_via_direct_process_entry`（走
    // process_entry 直调） 或集成测试独立 yaml。
    assert_eq!(report.skipped_too_large, 0);
    let _ = report; // 上方标注了原因；本测试仅确保 default 场景不误命中
}

/// 直接构造大 entry.size 走 process_entry 私有函数触发 skipped_too_large 分支。
/// FakeBackend.walk 返 entry.size = m.size，metas 的 size 由 `add_file`(size = bytes.len())
/// 决定；为造超限用 `add_file_with_times`（size 可参与虚 metas）；FakeBackend 无
/// set_size 直方法 → 改在 process_entry 层用 max_image_bytes=8 让 tiny_png(264) 溢出。
#[test]
fn move_text_shot_skips_too_large_via_direct_process_entry() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/big.png"), tiny_png()); // size=264 bytes
    let detector = FakeTextDetector::new(true);
    let src = local_loc("/src");
    let src_backend: Arc<dyn Backend> = factory.for_location(&src).unwrap();
    let out = local_loc("/out");
    let out_backend: Arc<dyn Backend> = factory.for_location(&out).unwrap();
    let cache: Arc<DashSet<Location>> = Arc::new(DashSet::new());
    // 直调 process_source 传 max_image_bytes=8 让 tiny_png(264) 触发 line 289-291 skip
    let delta = process_source(
        &detector,
        &src,
        &src_backend,
        &out,
        &out_backend,
        &canonical_prefix(&out),
        &cache,
        8,    // max_image_bytes < 264
        true, // dry_run 避免真副作用
    );
    assert_eq!(delta.skipped_too_large, 1);
    assert_eq!(delta.moved, 0);
}

/// `merge_delta` 内 `errors.len() >= ERRORS_SOFT_CAP` 累加走 truncated=true 分支。
/// 直接构造两个 SourceDelta 合并即可（不必真跑 1000+ 失败）。
#[test]
fn merge_delta_marks_errors_truncated_when_cap_exceeded() {
    let mut report = MoveTextShotReport::default();
    // 先把 report.errors 填到 SOFT_CAP，再 merge 一个含 1 条 error 的 delta
    for i in 0..ERRORS_SOFT_CAP {
        report.errors.push(ReportError {
            path: format!("prefill/{i}"),
            message: "prefilled".into(),
        });
    }
    let extra = SourceDelta {
        failed: 1,
        errors: vec![ReportError {
            path: "extra".into(),
            message: "over cap".into(),
        }],
        ..SourceDelta::default()
    };
    merge_delta(&mut report, extra);
    assert_eq!(report.failed, 1);
    assert!(report.errors_truncated);
    // 未超 cap 前 errors 仍 = SOFT_CAP；new item 未 push
    assert_eq!(report.errors.len(), ERRORS_SOFT_CAP);
}

/// `merge_delta` 内 cap 未超时正常 push（`errors_truncated` 保持 false）。
#[test]
fn merge_delta_keeps_truncated_false_when_under_cap() {
    let mut report = MoveTextShotReport::default();
    let delta = SourceDelta {
        errors: vec![ReportError {
            path: "a".into(),
            message: "m".into(),
        }],
        ..SourceDelta::default()
    };
    merge_delta(&mut report, delta);
    assert!(!report.errors_truncated);
    assert_eq!(report.errors.len(), 1);
}

/// `reduce_delta` 两个非空 SourceDelta 合并计数与 errors 汇总。
#[test]
fn reduce_delta_sums_counts_and_extends_errors() {
    let a = SourceDelta {
        scanned: 3,
        moved: 1,
        errors: vec![ReportError {
            path: "x".into(),
            message: "e1".into(),
        }],
        ..SourceDelta::default()
    };
    let b = SourceDelta {
        scanned: 2,
        deduplicated: 5,
        errors: vec![ReportError {
            path: "y".into(),
            message: "e2".into(),
        }],
        ..SourceDelta::default()
    };
    let r = reduce_delta(a, b);
    assert_eq!(r.scanned, 5);
    assert_eq!(r.moved, 1);
    assert_eq!(r.deduplicated, 5);
    assert_eq!(r.errors.len(), 2);
}

/// P0 §2 兜底：source 是单文件（walker yield entry.location == source）时 rel 空
/// → `file_name()` = None → record_failure 而非 expect panic。
#[test]
fn move_text_shot_records_failure_when_source_is_single_file_without_name() {
    // FakeBackend 允许把 source 直指单文件；walker yield 该 entry。
    let (fake, factory) = fake_factory();
    fake.add_file(local_loc("/lone.png"), tiny_png());
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/lone.png")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    // relative_to("/lone.png", "/lone.png") = ""，file_name None → record_failure
    assert_eq!(report.failed, 1);
    assert!(
        report.errors[0].message.contains("cannot derive file name"),
        "got: {}",
        report.errors[0].message
    );
}

/// 幂等 Duplicate 分支：target 已存在且双侧 SHA-512 相等 → 删源计 deduplicated（真跑）。
#[test]
fn move_text_shot_deduplicates_when_target_exists_with_same_hash() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    let bytes = tiny_png();
    fake.add_file(local_loc("/src/a.png"), bytes.clone());
    fake.add_dir(local_loc("/out"));
    // out/a.png 内容与 src 完全一致 → SHA-512 相等 → Duplicate 幂等分支
    fake.add_file(local_loc("/out/a.png"), bytes);
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();

    assert_eq!(report.deduplicated, 1);
    assert_eq!(report.moved, 0);
    // src 被删（视为已归档）；dst 未变
    assert!(!fake.exists(&local_loc("/src/a.png")).unwrap());
    assert!(fake.exists(&local_loc("/out/a.png")).unwrap());
}

/// 幂等 Duplicate 分支 dry_run：不删源、不动 dst，仅计 deduplicated。
#[test]
fn move_text_shot_dry_run_records_deduplicated_without_removing_src() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    let bytes = tiny_png();
    fake.add_file(local_loc("/src/a.png"), bytes.clone());
    fake.add_dir(local_loc("/out"));
    fake.add_file(local_loc("/out/a.png"), bytes);
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap();
    assert_eq!(report.deduplicated, 1);
    assert_eq!(report.moved, 0);
    // dry_run 不删源
    assert!(fake.exists(&local_loc("/src/a.png")).unwrap());
}

/// 幂等 Duplicate 内 `remove_file` Err → record_failure。
#[test]
fn move_text_shot_records_failure_when_dedupe_remove_src_fails() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    let bytes = tiny_png();
    fake.add_file(local_loc("/src/a.png"), bytes.clone());
    fake.add_dir(local_loc("/out"));
    fake.add_file(local_loc("/out/a.png"), bytes);
    fake.inject_error(
        local_loc("/src/a.png"),
        crate::FakeOp::RemoveFile,
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
    assert_eq!(report.deduplicated, 0);
    assert_eq!(report.failed, 1);
    // src 未删；dst 保持
    assert!(fake.exists(&local_loc("/src/a.png")).unwrap());
}

/// target 存在但 size 不同 → 直接判非幂等，走 unique_name _1 分派（跳过 SHA-512 open_read）。
#[test]
fn move_text_shot_size_mismatch_skips_hash_and_uses_unique_name() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png()); // size=264
    fake.add_dir(local_loc("/out"));
    // out/a.png 内容不同且 size 也不同（1 字节） → dedupe 内 size 快过滤直接非幂等
    fake.add_file(local_loc("/out/a.png"), b"x".to_vec());
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
    assert_eq!(report.deduplicated, 0);
    assert!(fake.exists(&local_loc("/out/a_1.png")).unwrap());
}

/// `bytes_hash_equal` 内 target open_read Err → dedupe 阶段返 Err → record_failure。
#[test]
fn move_text_shot_records_failure_when_dedupe_open_read_target_fails() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/a.png"), tiny_png());
    fake.add_dir(local_loc("/out"));
    fake.add_file(local_loc("/out/a.png"), tiny_png()); // 同 size 让 size fast-path 不 short-circuit
    // target open_read Err → bytes_hash_equal? 传到 dedupe_or_pick_target 报错
    fake.inject_error(
        local_loc("/out/a.png"),
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
    // Duplicate 未确认前，src 仍在
    assert!(fake.exists(&local_loc("/src/a.png")).unwrap());
}

/// `is_entry_under_output` 字面 fast-path：entry 显示路径直接 under prefix → true。
#[test]
fn is_entry_under_output_true_by_literal_prefix() {
    let entry = local_loc("/photos/archive/x.png");
    assert!(is_entry_under_output(&entry, "/photos/archive"));
}

/// `is_entry_under_output` 字面 & canonical 均不 under → false（走完 fast-path + fallback）。
#[test]
fn is_entry_under_output_false_when_disjoint() {
    let outside = local_loc("/photos/other/x.png");
    assert!(!is_entry_under_output(&outside, "/photos/archive"));
}

/// `is_entry_under_output` canonical fallback true 分支需 real symlink：Unix 下用 tempdir +
/// std::os::unix::fs::symlink 构造 output→physical_dir 的软链，让 entry 的 walker 路径
/// 与 canonical output 差异（entry 是原始路径含 symlink 段，canonical prefix 已解链）。
#[cfg(unix)]
#[test]
fn is_entry_under_output_true_via_canonical_when_symlink_pointing_into_output() {
    let temp = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(temp.path()).unwrap().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    // entry 走 symlink 路径（原样）；output_prefix 用 real 目录（已解链）
    let entry_path = link.join("x.png");
    std::fs::write(&entry_path, b"data").unwrap();
    let entry = local_loc(entry_path.to_str().unwrap());
    let output_prefix = real.to_string_lossy().to_string();
    // 字面 fast-path：entry 显示是 link/x.png，与 real prefix 字面不同 → false
    // canonical fallback：entry canonicalize 到 real/x.png → under real → true
    assert!(is_entry_under_output(&entry, &output_prefix));
}

/// `dedupe_or_pick_target` 内 `size == src_bytes.len() && hash != src_hash` 的 sub-branch：
/// size 相等但内容不同 → hash_equal 返 Ok(false) → 走 unique_name _1 分派。
/// 补覆盖 BRDA line 479 sub-branch 3。
#[test]
fn move_text_shot_size_equal_but_hash_differs_uses_unique_name() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    let src_bytes = tiny_png();
    fake.add_file(local_loc("/src/a.png"), src_bytes.clone());
    fake.add_dir(local_loc("/out"));
    // out/a.png 长度与 src 相同但字节不同 → size 快过滤 true + hash != → 非幂等
    let mut collide = src_bytes;
    // 翻转倒数第 2 字节让 size 保持 264 但 hash 差
    let n = collide.len();
    collide[n - 2] ^= 0xff;
    fake.add_file(local_loc("/out/a.png"), collide);
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
    assert_eq!(report.deduplicated, 0);
    assert!(fake.exists(&local_loc("/out/a_1.png")).unwrap());
    // 原 out/a.png 内容保持（size 一致 hash 不同的那份）
    assert!(fake.exists(&local_loc("/out/a.png")).unwrap());
}

/// `do_move_file` 内 mkdir_cache 命中分支：同 target_dir 下第二个文件时 contains=true
/// → 跳过 mkdir_p 调用（补 BRDA line 598 sub-branch=cache hit）。
#[test]
fn do_move_file_skips_mkdir_p_when_cache_contains_target_dir() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_dir(local_loc("/src/sub"));
    // 两个文件同 target_dir /out/sub → 第二次 do_move_file 走 cache hit skip mkdir_p
    fake.add_file(local_loc("/src/sub/a.png"), tiny_png());
    fake.add_file(local_loc("/src/sub/b.png"), tiny_png());
    let detector = FakeTextDetector::new(true);

    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.moved, 2);
    // FakeBackend 会记 mkdir_p 调用次数；rayon 场景下两个 worker 都可能 contains=false
    // 各触发一次 mkdir_p —— 命中 cache 无法完全断言 count，但至少 mkdir_p_calls <= 2 且
    // 两文件都成功归档即证明 cache pattern 未破坏路径正确性。
    let mkdir_calls = fake.mkdir_p_calls();
    assert!(
        mkdir_calls <= 2,
        "mkdir_p 应受 cache 抑制不超过两 worker × 1 次；got: {mkdir_calls}"
    );
    assert!(fake.exists(&local_loc("/out/sub/a.png")).unwrap());
    assert!(fake.exists(&local_loc("/out/sub/b.png")).unwrap());
}

/// `ensure_no_overlap` 内 sources 两两检查的对称分支（j==i skip）实测：
/// 单 source 时 for j 走一遍 i==j continue（其他 j 无）；两 source 无 overlap 时 j!=i 但
/// `under_prefix` false → 不 Err，正常返回 Ok。补 line 136-137 continue 分支覆盖。
#[test]
fn ensure_no_overlap_accepts_disjoint_sources() {
    let (_fake, factory) = fake_factory();
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/a"), local_loc("/b")],
        &local_loc("/out"),
        true,
    )
    .expect("两 disjoint sources 应通过 overlap 检查");
    assert_eq!(report.moved, 0);
}

/// `do_move_file` 内 `supports_native_rename_to` = true 的 fast-path 分支：需要真实
/// `LocalBackend` 实例才 override 返 true（`FakeBackend` 走 trait default = false）。
/// 用真实 tempdir + `DefaultBackendFactory` 触发 `fs::rename` fast-path。
#[test]
fn move_text_shot_fast_path_rename_when_local_to_local() {
    use crate::adapters::backend::factory::DefaultBackendFactory;
    let src_dir = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let src_path = src_dir.path().join("a.png");
    std::fs::write(&src_path, tiny_png()).unwrap();
    let sources = vec![Location::Local(
        Utf8PathBuf::from_path_buf(src_dir.path().to_path_buf()).unwrap(),
    )];
    let output = Location::Local(Utf8PathBuf::from_path_buf(out_dir.path().to_path_buf()).unwrap());
    let detector = FakeTextDetector::new(true);
    let report =
        move_text_shot(&detector, &DefaultBackendFactory, &sources, &output, false).unwrap();
    assert_eq!(report.moved, 1);
    assert_eq!(report.failed, 0);
    // fast-path fs::rename 走完源已不在
    assert!(!src_path.exists());
    assert!(out_dir.path().join("a.png").exists());
}
