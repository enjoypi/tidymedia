use camino::Utf8PathBuf;
use tempfile::tempdir;
use tidymedia::{run_cli, tidy, Commands, Location};

const DATA_DIR: &str = "tests/data";

fn local(p: &str) -> Location {
    Location::Local(Utf8PathBuf::from(p))
}

#[test]
fn tidy_dispatches_find_fast_on_data_dir() {
    tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: None,
    })
    .expect("find fast should succeed");
}

#[test]
fn tidy_dispatches_find_secure_on_data_dir() {
    tidy(Commands::Find {
        secure: true,
        sources: vec![local(DATA_DIR)],
        output: None,
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
    })
    .expect("move dry run should succeed");
}

/// SMB/MTP URI 当前未启用真实 client：CLI 解析成功（URI 语法正确）但 tidy
/// adapter 拒收，给出清晰 Unsupported 错误。
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
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("smb backend not enabled"), "got: {msg}");
}

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
    });
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("mtp backend not enabled"), "got: {msg}");
}

/// Copy 分支：sources 含非 Local Location → require_local_paths ? Err
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
    });
    assert!(format!("{}", res.unwrap_err()).contains("smb backend not enabled"));
}

/// Find 分支：output 是非 Local Location → option.map.transpose ? Err
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
    });
    assert!(format!("{}", res.unwrap_err()).contains("mtp backend not enabled"));
}

/// Move 分支：sources 非 Local → require_local_paths ? Err
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
    });
    assert!(format!("{}", res.unwrap_err()).contains("smb backend not enabled"));
}

/// Move 分支：output 非 Local → require_local_path ? Err
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
    });
    assert!(format!("{}", res.unwrap_err()).contains("mtp backend not enabled"));
}

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
