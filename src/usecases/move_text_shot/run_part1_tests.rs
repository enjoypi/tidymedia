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
