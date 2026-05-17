use std::collections::HashMap;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;
use tidymedia::{
    run_cli, tidy, tidy_with, Backend, BackendFactory, Commands, Error, FakeBackend, FakeOp,
    LocalBackend, Location, Result,
};

const DATA_DIR: &str = "tests/data";

fn local(p: &str) -> Location {
    Location::Local(Utf8PathBuf::from(p))
}

/// 集成测试用的 BackendFactory：local scheme 给真实 LocalBackend，其他 scheme
/// 从注入 map 取 Arc<dyn Backend>（通常是 FakeBackend）；未注入 scheme 返 Unsupported。
struct FakeBackendFactory {
    by_scheme: HashMap<&'static str, Arc<dyn Backend>>,
}

impl FakeBackendFactory {
    fn new() -> Self {
        Self {
            by_scheme: HashMap::new(),
        }
    }

    fn insert(&mut self, scheme: &'static str, backend: Arc<dyn Backend>) {
        self.by_scheme.insert(scheme, backend);
    }
}

impl BackendFactory for FakeBackendFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        if let Some(b) = self.by_scheme.get(loc.scheme()) {
            return Ok(Arc::clone(b));
        }
        if matches!(loc, Location::Local(_)) {
            return Ok(LocalBackend::arc());
        }
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("no fake backend for scheme {}", loc.scheme()),
        )))
    }
}

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn mtp_loc(path: &str) -> Location {
    Location::Mtp {
        device: "Pixel".into(),
        storage: "Internal".into(),
        path: Utf8PathBuf::from(path),
    }
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

// ===== Task 6：FakeBackendFactory 注入下的跨 scheme 调度集成测试 =====

#[test]
fn tidy_with_copy_fake_smb_to_local_writes_file() {
    let smb_root = smb_loc("");
    let smb_file = smb_loc("foo.jpg");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file.clone(), b"FAKE-MEDIA-BYTES".to_vec());

    let out_dir = tempdir().unwrap();
    let out_loc = local(out_dir.path().to_str().unwrap());

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![smb_root],
            output: out_loc,
        },
    )
    .expect("cross-backend copy smb->local should succeed");

    // FakeBackend metadata 给 UNIX_EPOCH（1970-01），valuable_name 为空（URI 各段都是 ASCII）。
    let target = out_dir.path().join("1970").join("01").join("foo.jpg");
    assert!(target.exists(), "expected copied file at {target:?}");
}

#[test]
fn tidy_with_find_mixed_local_smb_mtp_sources() {
    // 在每个 backend 各放一个 byte-pattern 不同的文件，find 应当跑通且无 panic。
    let smb_root = smb_loc("");
    let smb_file = smb_loc("a.bin");
    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file, vec![0xAA; 2048]);

    let mtp_root = mtp_loc("");
    let mtp_file = mtp_loc("b.bin");
    let fake_mtp = Arc::new(FakeBackend::new("mtp"));
    fake_mtp.add_dir(mtp_root.clone());
    fake_mtp.add_file(mtp_file, vec![0x55; 2048]);

    let local_src = tempdir().unwrap();
    std::fs::write(local_src.path().join("c.bin"), vec![0xCC; 2048]).unwrap();

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);
    factory.insert("mtp", fake_mtp);

    tidy_with(
        &factory,
        Commands::Find {
            secure: false,
            sources: vec![smb_root, mtp_root, local(local_src.path().to_str().unwrap())],
            output: None,
        },
    )
    .expect("find across mixed schemes should succeed");
}

#[test]
fn tidy_with_move_local_to_fake_mtp_removes_src() {
    // local 源放一个文件 → move 到 MTP，dry_run=false：copy + remove_file 都被触发。
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, b"hello mtp").unwrap();
    // 固定 mtime 让 create_time 桶稳定（2024-01-01 UTC）。
    let mtime = filetime::FileTime::from_unix_time(1_704_067_200, 0);
    filetime::set_file_mtime(&src_file, mtime).unwrap();

    let mtp_root = mtp_loc("dst");
    let fake_mtp = Arc::new(FakeBackend::new("mtp"));
    fake_mtp.add_dir(mtp_root.clone());

    let mut factory = FakeBackendFactory::new();
    factory.insert("mtp", Arc::clone(&fake_mtp) as Arc<dyn Backend>);

    tidy_with(
        &factory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: mtp_root,
        },
    )
    .expect("local -> mtp move should succeed");

    assert!(!src_file.exists(), "src must be removed after move");
    // 固定 mtime 落在北京时区 2024-01-01 08:00 → 2024/01 桶。
    let dst_loc = mtp_loc("dst/2024/01/photo.bin");
    assert!(
        fake_mtp.read_bytes(&dst_loc).is_some(),
        "expected fake mtp to hold copied file at dst/2024/01/photo.bin"
    );
}

#[test]
fn tidy_with_propagates_smb_open_read_error() {
    // FakeBackend 注入 OpenRead Err：visit_location 阶段 Info::open 内部就 fail，
    // 文件被计为 skipped_unreadable；copy 整体仍返 Ok（同 LocalBackend 下 chmod 000 语义）。
    let smb_root = smb_loc("");
    let smb_file = smb_loc("locked.bin");
    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file.clone(), vec![1; 1024]);
    fake_smb.inject_error(smb_file, FakeOp::OpenRead, std::io::ErrorKind::PermissionDenied);

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![smb_root],
            output: local(out_dir.path().to_str().unwrap()),
        },
    )
    .expect("copy should still return Ok with skipped_unreadable stat");

    assert!(
        std::fs::read_dir(out_dir.path()).unwrap().next().is_none(),
        "output must be empty when source unreadable"
    );
}

#[test]
fn tidy_with_propagates_smb_walk_error() {
    // FakeBackend 注入 Walk Err：visit_location 走 walker_errors 分支，仍 Ok 返回。
    let smb_root = smb_loc("");
    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.inject_error(smb_root.clone(), FakeOp::Walk, std::io::ErrorKind::PermissionDenied);

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    tidy_with(
        &factory,
        Commands::Find {
            secure: true,
            sources: vec![smb_root],
            output: Some(local(out_dir.path().to_str().unwrap())),
        },
    )
    .expect("find should swallow walker error and finalize Ok");
}

#[test]
fn default_factory_smb_without_feature_returns_unsupported() {
    // 默认 BackendFactory 在未启用 smb-backend feature 时拒收 SMB Location。
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![smb_loc("photos")],
        output: None,
    });
    let err = res.unwrap_err();
    assert!(
        format!("{err}").contains("smb backend not enabled"),
        "expected unsupported, got: {err}"
    );
}
