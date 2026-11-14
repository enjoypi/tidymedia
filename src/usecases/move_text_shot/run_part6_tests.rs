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

#[derive(Debug)]
struct BudgetExceededDetector;

impl crate::usecases::ocr::TextDetector for BudgetExceededDetector {
    fn has_text(&self, _path: &camino::Utf8Path, _image_bytes: &[u8]) -> io::Result<bool> {
        Err(io::Error::new(io::ErrorKind::FileTooLarge, "inject"))
    }
}

#[test]
fn detector_budget_overflow_counts_as_skipped_too_large() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/huge.png"), tiny_png());
    let report = move_text_shot(
        &BudgetExceededDetector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap();
    assert_eq!(report.skipped_too_large, 1);
    assert_eq!(report.failed, 0);
}

fn mapped_report(unchanged: bool) -> MoveTextShotReport {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/shot.png"), tiny_png());
    fake.add_file(local_loc("/src/note.txt"), b"plain text".to_vec());
    fake.set_mappable(unchanged);
    let detector = FakeTextDetector::new(true);
    move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap()
}

#[test]
fn mapped_sources_are_sniffed_and_moved_like_streamed_ones() {
    let report = mapped_report(true);
    assert_eq!((report.moved, report.skipped_non_image), (1, 1));
    assert_eq!(report.failed, 0);
}

#[test]
fn mapped_source_changed_while_reading_is_reported_as_failure() {
    let report = mapped_report(false);
    assert_eq!((report.moved, report.failed), (0, 1));
    assert!(report.errors[0].message.contains("changed while"));
}

#[test]
fn mapping_failure_is_reported_as_failure() {
    let (fake, factory) = fake_factory();
    fake.add_dir(local_loc("/src"));
    fake.add_file(local_loc("/src/shot.png"), tiny_png());
    fake.set_mappable(true);
    fake.inject_error(
        local_loc("/src/shot.png"),
        crate::FakeOp::OpenRead,
        io::ErrorKind::PermissionDenied,
    );
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
}

#[test]
fn walk_is_processed_in_bounded_chunks() {
    use crate::entities::backend::{Entry, EntryKind};

    let entries = (0..6).map(|i| {
        if i == 5 {
            return Err(io::Error::other("walker"));
        }
        Ok(Entry {
            location: local_loc(&format!("/src/{i}.png")),
            size: 1,
            kind: EntryKind::File,
        })
    });
    let sizes = std::sync::Mutex::new(Vec::new());
    let total = crate::usecases::move_text_shot::scan::in_chunks(entries, 2, "/src", |chunk| {
        sizes.lock().unwrap().push(chunk.len());
        crate::usecases::move_text_shot::delta::SourceDelta {
            scanned: chunk.len(),
            ..Default::default()
        }
    });
    assert_eq!(*sizes.lock().unwrap(), [2, 2, 1]);
    assert_eq!((total.scanned, total.failed), (5, 1));
}
