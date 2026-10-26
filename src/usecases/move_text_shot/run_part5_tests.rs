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

/// 用户输入兜底：source 是单文件（walker yield entry.location == source）时 rel 空
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
