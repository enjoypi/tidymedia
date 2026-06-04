//! `tidy_with` 走 `FakeBackendFactory` 注入的远端 backend 业务测试。
//! 从 `tests/lib_tidy.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::sync::Arc;

use tempfile::tempdir;
use tidymedia::{Backend, Commands, FakeBackend, FakeOp, tidy, tidy_with};

use super::{FakeBackendFactory, adb_loc, local, mtp_loc, smb_loc};

#[test]
fn tidy_with_copy_fake_smb_to_local_writes_file() {
    let smb_root = smb_loc("");
    let smb_file = smb_loc("foo.jpg");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file, b"FAKE-MEDIA-BYTES".to_vec());

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
            archive_template: None,
            report: None,
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
            sources: vec![
                smb_root,
                mtp_root,
                local(local_src.path().to_str().unwrap()),
            ],
            output: None,
            report: None,
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
            archive_template: None,
            report: None,
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

// duplicate + Move (remove=true) + dry_run=false：实际删除源。触发 L176:22
// `!dry_run` 的 True 分支（在 lib_tidy 集成 binary instance 上同时覆盖 T 与 F）。
#[test]
fn tidy_move_with_duplicate_removes_src_when_not_dry_run() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let payload = vec![0x77u8; 4096];
    let src_file = src_dir.path().join("photo2.bin");
    std::fs::write(&src_file, &payload).unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_704_067_200, 0);
    filetime::set_file_mtime(&src_file, mtime).unwrap();
    std::fs::write(out_dir.path().join("dup2.bin"), &payload).unwrap();

    tidy(Commands::Move {
        dry_run: false,
        include_non_media: true,
        sources: vec![local(src_dir.path().to_str().unwrap())],
        output: local(out_dir.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("move with duplicate should succeed");

    assert!(
        !src_file.exists(),
        "real move must remove src when duplicate detected"
    );
}

// 触发 `do_copy` 中 duplicate 检测 + remove + dry_run 的三态 branch：
// `if remove && !dry_run` 的 `!dry_run` False 分支（即 dry_run=true 时不实际删源）。
#[test]
fn tidy_move_dry_run_with_duplicate_skips_actual_remove() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    // 同一内容写两份：source 与 output 各一个，SHA-512 相等 → duplicate 命中。
    let payload = vec![0x42u8; 4096];
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, &payload).unwrap();
    // 固定 mtime 让 create_time 桶稳定，与 lib_tidy 其他用例一致。
    let mtime = filetime::FileTime::from_unix_time(1_704_067_200, 0);
    filetime::set_file_mtime(&src_file, mtime).unwrap();
    // 把同样内容放到 output 任意位置，让 output_index 扫描时挂上同 hash 文件。
    std::fs::write(out_dir.path().join("dup.bin"), &payload).unwrap();

    tidy(Commands::Move {
        dry_run: true,
        include_non_media: true,
        sources: vec![local(src_dir.path().to_str().unwrap())],
        output: local(out_dir.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("dry-run move with duplicate should succeed");

    // dry_run=true 时即便检测到 duplicate 也不应删源。
    assert!(
        src_file.exists(),
        "dry-run must not remove src when duplicate detected"
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
    fake_smb.inject_error(
        smb_file,
        FakeOp::OpenRead,
        std::io::ErrorKind::PermissionDenied,
    );

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
            archive_template: None,
            report: None,
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
    fake_smb.inject_error(
        smb_root.clone(),
        FakeOp::Walk,
        std::io::ErrorKind::PermissionDenied,
    );

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    tidy_with(
        &factory,
        Commands::Find {
            secure: true,
            sources: vec![smb_root],
            output: Some(local(out_dir.path().to_str().unwrap())),
            report: None,
        },
    )
    .expect("find should swallow walker error and finalize Ok");
}

#[cfg(not(feature = "smb-backend"))]
#[test]
fn default_factory_smb_without_feature_returns_unsupported() {
    // 默认 BackendFactory 在未启用 smb-backend feature 时拒收 SMB Location。
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![smb_loc("photos")],
        output: None,
        report: None,
    });
    let err = res.unwrap_err();
    assert!(
        format!("{err}").contains("smb backend not enabled"),
        "expected unsupported, got: {err}"
    );
}

#[cfg(not(feature = "adb-backend"))]
#[test]
fn default_factory_adb_without_feature_returns_unsupported() {
    let res = tidy(Commands::Find {
        secure: false,
        sources: vec![adb_loc("/sdcard/DCIM")],
        output: None,
        report: None,
    });
    let err = res.unwrap_err();
    assert!(
        format!("{err}").contains("adb backend not enabled"),
        "expected unsupported, got: {err}"
    );
}

#[test]
fn tidy_with_copy_fake_adb_to_local_writes_file() {
    // 模拟 PC 端从手机 ADB 把照片整理到本地：fake adb 提供文件，tidy 读后写入本地输出。
    let adb_root = adb_loc("/sdcard/DCIM");
    let adb_file = adb_loc("/sdcard/DCIM/foo.jpg");

    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.add_file(adb_file, b"FAKE-PHOTO-BYTES".to_vec());

    let out_dir = tempdir().unwrap();
    let out_loc = local(out_dir.path().to_str().unwrap());

    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", fake_adb);

    tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![adb_root],
            output: out_loc,
            archive_template: None,
            report: None,
        },
    )
    .expect("adb -> local copy should succeed");

    // FakeBackend metadata 走 UNIX_EPOCH → 1970/01 桶。
    let target = out_dir.path().join("1970").join("01").join("foo.jpg");
    assert!(target.exists(), "expected copied file at {target:?}");
}

#[test]
fn tidy_with_find_mixed_local_smb_adb_sources() {
    let smb_root = smb_loc("");
    let smb_file = smb_loc("a.bin");
    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file, vec![0xAA; 2048]);

    let adb_root = adb_loc("/sdcard");
    let adb_file = adb_loc("/sdcard/b.bin");
    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.add_file(adb_file, vec![0x55; 2048]);

    let local_src = tempdir().unwrap();
    std::fs::write(local_src.path().join("c.bin"), vec![0xCC; 2048]).unwrap();

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);
    factory.insert("adb", fake_adb);

    tidy_with(
        &factory,
        Commands::Find {
            secure: false,
            sources: vec![
                smb_root,
                adb_root,
                local(local_src.path().to_str().unwrap()),
            ],
            output: None,
            report: None,
        },
    )
    .expect("find across smb/adb/local should succeed");
}

#[test]
fn tidy_with_move_local_to_fake_adb_removes_src() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, b"hello adb").unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_704_067_200, 0);
    filetime::set_file_mtime(&src_file, mtime).unwrap();

    let adb_root = adb_loc("/sdcard/Inbox");
    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());

    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", Arc::clone(&fake_adb) as Arc<dyn Backend>);

    tidy_with(
        &factory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: adb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("local -> adb move should succeed");

    assert!(!src_file.exists(), "src must be removed after move");
    let dst_loc = adb_loc("/sdcard/Inbox/2024/01/photo.bin");
    assert!(
        fake_adb.read_bytes(&dst_loc).is_some(),
        "expected fake adb to hold copied file at /sdcard/Inbox/2024/01/photo.bin"
    );
}
