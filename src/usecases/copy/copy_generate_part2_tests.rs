// output 扫描遇空文件（不入索引，skipped_empty>0）→ 索引非权威 → 保留磁盘探测：
// 同名空文件让位 _1 后缀而非被覆盖（安全网端到端回归）。
#[test]
fn copy_probes_disk_when_output_scan_skipped_files() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out = tempdir().unwrap();
    let sub = out.path().join("2024").join("01");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("photo.png"), b"").unwrap();
    let report = copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        false,
        Some(DEFAULT_TMPL),
        None,
    )
    .unwrap();
    assert_eq!(report.copied, 1);
    assert!(sub.join("photo_1.png").exists());
    let empty_len = fs::metadata(sub.join("photo.png")).unwrap().len();
    assert_eq!(empty_len, 0);
}

// dry-run 且 output 根不存在（首次归档最常见）：跳过 output walk（索引权威、
// 零探测 RTT），报告正常 copied 且磁盘无写入。
#[test]
fn copy_dry_run_with_missing_output_root_succeeds() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out = tempdir().unwrap();
    let missing = out.path().join("not-yet-created");
    let report = copy(
        &[local_source(src.path())],
        (local_loc(&missing), local_arc()),
        true,
        false,
        false,
        Some(DEFAULT_TMPL),
        None,
    )
    .unwrap();
    assert_eq!(report.copied, 1);
    assert!(!missing.exists());
}

#[test]
fn do_copy_errors_when_unique_name_exhausted() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    fill_collisions(&out.path().join("2024").join("01"));
    let idx = crate::entities::file_index::Index::new();
    let err = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .expect_err("must error after collisions");
    assert!(err.to_string().contains("无法为"));
}

#[test]
fn copy_logs_failure_when_target_collisions_exhausted() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out = tempdir().unwrap();
    fill_collisions(&out.path().join("2024").join("01"));
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        false,
        None,
        None,
    )
    .unwrap();
}

#[test]
fn do_copy_dry_run_reports_target_but_writes_nothing() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    let idx = crate::entities::file_index::Index::new();
    let opts = CopyOpts {
        dry_run: true,
        remove: false,
        include_non_media: false,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let did_copy = do_copy(&info, &local_loc(out.path()), &local_arc(), &idx, &opts).unwrap();
    assert!(did_copy);
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

// 启用 trace 级别 subscriber，让 copy() 里的 trace! 宏闭包被求值，覆盖 L62 region。
#[test]
fn copy_with_trace_subscriber_executes_trace_branch() {
    use tracing_subscriber::EnvFilter;
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("trace"))
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            true,
            false,
            false,
            None,
            None,
        )
        .unwrap();
    });
}

// `..` / `.` 路径段（恶意 EXIF 字段或自定义模板静态文本）必须被 naming::filter
// 过滤掉；Utf8PathBuf::join("..") 是字面拼接不规范化，让 fs::File::create 按字面
// 解析致 output 父目录被写入。本测试用自定义模板 `{year}/../target/.` 模拟该
// 注入向量，验证逐段剥除后 sub_dir 落在 out/<year>/target 而非 out 父目录。
#[test]
fn generate_unique_name_drops_dot_and_dotdot_segments() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let template = "{year}/../target/.";
    let (sub_dir, target) = generate_unique_name(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        template,
        &Index::new(),
        false,
    )
    .unwrap()
    .expect("path generated");
    let sub_dir_str = sub_dir.path().as_str();
    let out_str = utf8(out.path());
    assert!(
        sub_dir_str.starts_with(out_str.as_str()),
        "subdir {sub_dir_str} must stay under output {out_str}"
    );
    assert!(
        !sub_dir_str.contains(".."),
        "subdir must not contain dotdot, got: {sub_dir_str}"
    );
    assert!(target.path().as_str().starts_with(out_str.as_str()));
    assert!(
        std::path::Path::new(target.path().as_str()).ends_with("target/photo.png"),
        "unexpected target tail: {}",
        target.path()
    );
}

// output 是已存在文件（非目录），backend.mkdir_p 失败 → 覆盖 copy() 内 `?` Err 分支。
#[test]
fn copy_with_output_as_file_errors() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out_file = tempfile::NamedTempFile::new().unwrap();
    let out_loc = Location::Local(Utf8PathBuf::from(out_file.path().to_str().unwrap()));
    let err = copy(
        &[local_source(src.path())],
        (out_loc, local_arc()),
        false,
        false,
        false,
        None,
        None,
    )
    .unwrap_err();
    let _ = err;
}
