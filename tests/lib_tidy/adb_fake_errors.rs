//! ADB Fake backend 错误注入：模拟手册流程 A 中 USB 抖断/权限失败的单文件不阻断其他。
//!
//! ADB backend 当前是 non-streaming（整文件读入内存）且 timeout 配置不生效（见
//! `src/adapters/backend/adb_real.rs`）。Fake 层模拟这些失败模式，验证业务路径
//! 的失败隔离语义，不依赖真机。

use std::sync::Arc;

use tempfile::tempdir;
use tidymedia::{CommandResult, Commands, FakeBackend, FakeOp, tidy_with};

use super::{FakeBackendFactory, adb_loc, local};

// 模拟 USB 抖断：`open_read` 成功但 reader 在 read 阶段立即 Err。
// 这是手册 §"已知工具限制"提到的 ADB 卡死失败模式之一。
#[test]
fn adb_copy_reader_error_treats_file_as_skipped() {
    let adb_root = adb_loc("/sdcard/DCIM");
    let adb_file = adb_loc("/sdcard/DCIM/flaky.jpg");

    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.add_file(adb_file.clone(), vec![0x11; 8192]);
    fake_adb.inject_reader_error(adb_file, std::io::ErrorKind::TimedOut);

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", fake_adb);

    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![adb_root],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy should swallow reader error and finalize Ok");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert!(
        report.skipped_unreadable >= 1,
        "reader error must be counted in skipped_unreadable: {report:?}"
    );
    assert!(
        std::fs::read_dir(out_dir.path()).unwrap().next().is_none(),
        "no file should land in out when source unreadable"
    );
}

// 多文件混合：其中一个 read 失败不阻断其他。手册 §A3 「拆目录单跑」的隐含契约：
// 即便不拆，单文件失败也只丢自己。
#[test]
fn adb_copy_mixed_files_one_failure_does_not_block_others() {
    let adb_root = adb_loc("/sdcard/DCIM");
    let good_file = adb_loc("/sdcard/DCIM/good.jpg");
    let bad_file = adb_loc("/sdcard/DCIM/bad.jpg");

    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.add_file(good_file, b"GOOD-BYTES-1234567890".to_vec());
    fake_adb.add_file(bad_file.clone(), b"BAD-BYTES-1234567890".to_vec());
    fake_adb.inject_error(
        bad_file,
        FakeOp::OpenRead,
        std::io::ErrorKind::PermissionDenied,
    );

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", fake_adb);

    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![adb_root],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy should continue past per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    // good.jpg 应当被 copy 出来（FakeBackend metadata UNIX_EPOCH → 1970/01）
    let good_dst = out_dir.path().join("1970").join("01").join("good.jpg");
    assert!(
        good_dst.exists(),
        "good file must reach out: {good_dst:?}; report={report:?}"
    );
    assert!(
        report.skipped_unreadable >= 1,
        "bad file must be counted as skipped_unreadable: {report:?}"
    );
}

// Find 模式 walker 注入 Err：保持与 SMB `tidy_with_propagates_smb_walk_error` 对称，
// 验证 ADB scheme 走相同的 walker_errors 容错路径。
#[test]
fn adb_find_walk_error_is_swallowed() {
    let adb_root = adb_loc("/sdcard/DCIM");
    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.inject_error(
        adb_root.clone(),
        FakeOp::Walk,
        std::io::ErrorKind::PermissionDenied,
    );

    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", fake_adb);

    tidy_with(
        &factory,
        Commands::Find {
            secure: false,
            sources: vec![adb_root],
            output: None,
            report: None,
        },
    )
    .expect("find should swallow walker error and finalize Ok");
}
