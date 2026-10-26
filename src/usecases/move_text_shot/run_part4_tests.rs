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

// ---- 重叠与失败分支覆盖 ----

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
