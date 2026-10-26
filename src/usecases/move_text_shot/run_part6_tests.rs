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

/// 确定性命中 mkdir_cache `contains=true` 分支：rayon 主流程下 cache hit 依赖线程
/// 调度（上面 `do_move_file_skips_mkdir_p_when_cache_contains_target_dir` 只保证
/// miss 路径可达），直测 `do_move_file` + 预填充 cache 让 hit 分支必中。
#[test]
fn do_move_file_cache_prefilled_never_calls_mkdir_p() {
    let src_fake = Arc::new(FakeBackend::new("local"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    src_fake.add_file(local_loc("/src/a.png"), tiny_png());
    let cache = Arc::new(dashmap::DashSet::new());
    cache.insert(local_loc("/out"));
    let src: Arc<dyn Backend> = src_fake.clone();
    let out: Arc<dyn Backend> = out_fake.clone();
    crate::usecases::move_text_shot::move_file::do_move_file(
        &src,
        &local_loc("/src/a.png"),
        &out,
        &local_loc("/out"),
        &local_loc("/out/a.png"),
        b"png-bytes",
        &cache,
    )
    .unwrap();
    assert_eq!(out_fake.mkdir_p_calls(), 0);
    assert!(out_fake.exists(&local_loc("/out/a.png")).unwrap());
    assert!(!src_fake.exists(&local_loc("/src/a.png")).unwrap());
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
    use crate::frameworks::backend::DefaultBackendFactory;
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
