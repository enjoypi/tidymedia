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
