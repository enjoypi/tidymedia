//! `tidy` / `run_cli` 调度 + scheme rejection 测试。
//! 从 `tests/lib_tidy.rs` 拆出避免单文件 > 512 行（P0 §6）。

#[cfg(not(all(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
)))]
use camino::Utf8PathBuf;
use tempfile::tempdir;
#[cfg(not(all(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
)))]
use tidymedia::Location;
use tidymedia::{Commands, run_cli, tidy, tidy_with};

use super::FakeBackendFactory;
#[cfg(not(feature = "adb-backend"))]
use super::adb_loc;
use super::{DATA_DIR, local};

#[test]
fn tidy_dispatches_find_fast_on_data_dir() {
    tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: None,
        report: None,
    })
    .expect("find fast should succeed");
}

#[test]
fn tidy_dispatches_find_secure_on_data_dir() {
    tidy(Commands::Find {
        secure: true,
        sources: vec![local(DATA_DIR)],
        output: None,
        report: None,
    })
    .expect("find secure should succeed");
}

#[test]
fn tidy_dispatches_find_with_output_directory() {
    let out = tempdir().unwrap();
    tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: Some(local(out.path().to_str().unwrap())),
        report: None,
    })
    .expect("find with output should succeed");
}

#[test]
fn tidy_dispatches_copy_dry_run_on_data_dir() {
    let out = tempdir().unwrap();
    tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("copy dry run should succeed");
}

#[test]
fn tidy_dispatches_move_dry_run_on_empty_source() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("move dry run should succeed");
}

/// Copy --report 路径：dispatch 在 Some(path) 下构造 `JsonFileReportSink` 并把
/// `&dyn ReportSink` 闭包转换喂给 use case；覆盖 `dispatch.rs` 的 sink 走 Some 分支。
#[test]
fn tidy_dispatches_copy_with_report_writes_json() {
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("copy_report.json");
    tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: Some(report_path.to_str().unwrap().to_string()),
    })
    .expect("copy with report should succeed");
    assert!(report_path.exists(), "report should be written");
}

/// Move --report 路径：与上同理覆盖 `Commands::Move` 分支的 sink 装配。
#[test]
fn tidy_dispatches_move_with_report_writes_json() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("move_report.json");
    tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: Some(report_path.to_str().unwrap().to_string()),
    })
    .expect("move with report should succeed");
    assert!(report_path.exists(), "report should be written");
}

/// SMB/MTP URI 当前未启用真实 client：CLI 解析成功（URI 语法正确）但 tidy
/// adapter 拒收，给出清晰 Unsupported 错误。
#[cfg(not(feature = "smb-backend"))]
#[test]
fn tidy_rejects_smb_uri_with_clear_error() {
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "photos".into(),
            path: Utf8PathBuf::new(),
        }],
        output: None,
        report: None,
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("smb backend not enabled"), "got: {msg}");
}

#[cfg(not(feature = "mtp-backend"))]
#[test]
fn tidy_rejects_mtp_output_with_clear_error() {
    let res = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: Location::Mtp {
            device: "Pixel 8".into(),
            storage: "Internal".into(),
            path: Utf8PathBuf::new(),
        },
        archive_template: None,
        report: None,
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("mtp backend not enabled"), "got: {msg}");
}

#[cfg(not(feature = "adb-backend"))]
#[test]
fn tidy_rejects_adb_uri_with_clear_error() {
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![adb_loc("/sdcard/DCIM")],
        output: None,
        report: None,
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("adb backend not enabled"), "got: {msg}");
}

#[cfg(not(feature = "adb-backend"))]
#[test]
fn tidy_rejects_adb_output_with_clear_error() {
    let res = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: adb_loc("/sdcard/Out"),
        archive_template: None,
        report: None,
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("adb backend not enabled"), "got: {msg}");
}

/// Copy 分支：sources 含非 Local Location → `require_local_paths` ? Err
#[cfg(not(feature = "smb-backend"))]
#[test]
fn tidy_rejects_copy_smb_source() {
    let out = tempdir().unwrap();
    let res = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::new(),
        }],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    });
    assert!(format!("{}", res.unwrap_err()).contains("smb backend not enabled"));
}

/// Find 分支：output 是非 Local Location → option.map.transpose ? Err
#[cfg(not(feature = "mtp-backend"))]
#[test]
fn tidy_rejects_find_mtp_output() {
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: Some(Location::Mtp {
            device: "d".into(),
            storage: "s".into(),
            path: Utf8PathBuf::new(),
        }),
        report: None,
    });
    assert!(format!("{}", res.unwrap_err()).contains("mtp backend not enabled"));
}

/// Move 分支：sources 非 Local → `require_local_paths` ? Err
#[cfg(not(feature = "smb-backend"))]
#[test]
fn tidy_rejects_move_smb_source() {
    let out = tempdir().unwrap();
    let res = tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![Location::Smb {
            user: None,
            host: "h".into(),
            port: None,
            share: "s".into(),
            path: Utf8PathBuf::new(),
        }],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    });
    assert!(format!("{}", res.unwrap_err()).contains("smb backend not enabled"));
}

/// Move 分支：output 非 Local → `require_local_path` ? Err
#[cfg(not(feature = "mtp-backend"))]
#[test]
fn tidy_rejects_move_mtp_output() {
    let src = tempdir().unwrap();
    let res = tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: Location::Mtp {
            device: "d".into(),
            storage: "s".into(),
            path: Utf8PathBuf::new(),
        },
        archive_template: None,
        report: None,
    });
    assert!(format!("{}", res.unwrap_err()).contains("mtp backend not enabled"));
}
#[cfg(not(feature = "smb-backend"))]
#[test]
fn run_cli_accepts_uri_form_smb_and_reports_unsupported() {
    let r = run_cli(["tidymedia", "find", "smb://nas/photos"]);
    let err = r.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("smb backend not enabled"), "got: {msg}");
}

#[test]
fn run_cli_find_subcommand_executes() {
    run_cli(["tidymedia", "find", DATA_DIR]).expect("find via run_cli should succeed");
}

// output 指向普通文件 → find_duplicates 的 not_a_directory 校验 Err，
// 触发 dispatch_find 内 `find_duplicates(..)?` 的 Err arm（dispatch.rs:116）。
#[test]
fn tidy_find_propagates_error_when_output_is_file() {
    let root = tempdir().unwrap();
    let blocker = root.path().join("file_not_dir");
    std::fs::write(&blocker, b"i am a file").unwrap();

    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: Some(local(blocker.to_str().unwrap())),
        report: None,
    });
    assert!(res.is_err(), "find output must be an existing directory");
}

// output 路径不存在（NotFound）→ find_duplicates 仍以 "not a directory" 文案 Err。
// 配合 lib_tidy 集成 binary instance 也覆盖 find 内 NotFound match guard arm，避免
// multi-binary instance 0-hit region miss。
#[test]
fn tidy_find_propagates_error_when_output_missing() {
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: Some(local("/no/such/dir/xyz")),
        report: None,
    });
    let err = res.unwrap_err();
    assert!(err.to_string().contains("not a directory"), "got: {err}");
}

// 直接调 internal helper 让 lib_tidy binary instance 也触发覆盖率敏感分支
// （multi-binary instance 下避免 0-hit region miss）。两条都是 lib unit 已覆盖、
// 仅 lib_tidy binary instance 缺 hit 的 region。
#[test]
fn compute_output_prefix_local_fallback_hits_lib_tidy_instance() {
    // 相对路径 + 不存在 → `full_path` 走 canonicalize_utf8 失败 → Err arm 触发。
    let pair: (tidymedia::Location, std::sync::Arc<dyn tidymedia::Backend>) = (
        tidymedia::Location::Local(camino::Utf8PathBuf::from("no_such_relative_dir_xyz_abc2")),
        tidymedia::LocalBackend::arc(),
    );
    let prefix = tidymedia::__compute_output_prefix(Some(&pair)).expect("Some");
    assert_eq!(prefix, "no_such_relative_dir_xyz_abc2");
}

#[test]
fn parse_xmp_dates_key_at_end_hits_lib_tidy_instance() {
    // 触发 find_attr_rfc3339 中 `let Some(quote) = chars().next() else continue;` arm。
    let xml = " photoshop:DateCreated=";
    let _ = tidymedia::__parse_xmp_dates(xml);
}

// output metadata 返非 NotFound 错误（PermissionDenied / 网络等）→ find 必须传播
// 原 Err，不被吞成 "not a directory"。FakeBackend inject_error 模拟该情况。
// 覆盖 lib_tidy binary instance 中 find_duplicates Err(other) arm，消除 multi-
// binary 0-hit region miss。
#[test]
fn tidy_find_propagates_permission_denied_metadata_error() {
    let smb_out = tidymedia::Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: camino::Utf8PathBuf::from("out"),
    };
    let fake_smb = std::sync::Arc::new(tidymedia::FakeBackend::new("smb"));
    fake_smb.add_dir(smb_out.clone());
    fake_smb.inject_error(
        smb_out.clone(),
        tidymedia::FakeOp::Metadata,
        std::io::ErrorKind::PermissionDenied,
    );

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb as std::sync::Arc<dyn tidymedia::Backend>);

    let err = tidy_with(
        &factory,
        Commands::Find {
            secure: false,
            sources: vec![local(DATA_DIR)],
            output: Some(smb_out),
            report: None,
        },
    )
    .unwrap_err();
    // 关键：错误不应被改写成 "not a directory"。
    assert!(
        !err.to_string().contains("not a directory"),
        "PermissionDenied must propagate, got: {err}"
    );
}

// output 父路径被普通文件占住 → usecases::copy 内 mkdir_p Err，
// 触发 dispatch_copy_or_move 内 `usecases::copy(..)?` 的 Err arm（line 101）。
// 所有 feature 组合下都跑（不依赖 backend feature gate）。
#[test]
fn tidy_copy_propagates_mkdir_error_when_output_parent_is_file() {
    let root = tempdir().unwrap();
    let blocker = root.path().join("file_not_dir");
    std::fs::write(&blocker, b"i am a file").unwrap();
    let bad_out = blocker.join("sub");

    let res = tidy(Commands::Copy {
        dry_run: false,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: local(bad_out.to_str().unwrap()),
        archive_template: None,
        report: None,
    });
    assert!(res.is_err(), "mkdir_p must fail when parent is a file");
}

#[test]
fn run_cli_help_exits_with_ok() {
    run_cli(["tidymedia", "--help"]).expect("help should return Ok");
}

#[test]
fn run_cli_version_exits_with_ok() {
    run_cli(["tidymedia", "--version"]).expect("version should return Ok");
}

#[test]
fn run_cli_unknown_subcommand_returns_err() {
    let r = run_cli(["tidymedia", "definitely-not-a-subcommand"]);
    assert!(r.is_err(), "unknown subcommand must return Err");
}

#[test]
fn run_cli_copy_dry_run_dispatches() {
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--dry-run",
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("copy --dry-run via run_cli should succeed");
}

#[test]
fn run_cli_move_dry_run_dispatches() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--dry-run",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --dry-run via run_cli should succeed");
}

#[test]
fn run_cli_find_secure_dispatches() {
    run_cli(["tidymedia", "find", "--secure", DATA_DIR])
        .expect("find --secure via run_cli should succeed");
}

#[test]
fn run_cli_copy_include_non_media_dispatches() {
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--dry-run",
        "--include-non-media",
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("copy --include-non-media via run_cli should succeed");
}

#[test]
fn run_cli_move_include_non_media_dispatches() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--dry-run",
        "--include-non-media",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --include-non-media via run_cli should succeed");
}
