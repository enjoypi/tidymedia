//! Move 故障恢复语义。`do_copy` 在 `OpenWrite` / `RemoveFile` 失败时 src 必须保留。
//! local→local move 命中 fast-path 走 `Backend::rename`（`fs::rename` 同卷原子，
//! 跨卷 fallback 到 copy+remove）；跨 backend（含本文件 `FakeBackend` 注入测试）仍走
//! `stream_copy + remove_file`。本文件用 `FakeBackend` 跨 backend 测试，
//! 不命中 fast-path，覆盖 stream 路径各阶段失败的"src 不丢"契约，
//! 对应验收手册 §B3「中断处理」流程的可观察行为。

use std::sync::Arc;

use tempfile::tempdir;
use tidymedia::{Backend, CommandResult, Commands, FakeBackend, FakeOp, tidy_with};

use super::{DATA_DIR, FakeBackendFactory, local, smb_loc};

// 注入 mtime = 2024-01-01 00:00:00 UTC → +8 时区落 2024/01 桶。
fn fix_mtime(path: &std::path::Path) {
    let mtime = filetime::FileTime::from_unix_time(1_704_067_200, 0);
    filetime::set_file_mtime(path, mtime).expect("set mtime");
}

// 跨 backend move：目标 OpenWrite Err → stream_copy 失败，src 必须保留。
#[test]
fn move_keeps_src_when_target_open_write_fails() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, vec![0xAA; 4096]).unwrap();
    fix_mtime(&src_file);

    let smb_root = smb_loc("dst");
    // do_copy 实际目标路径 = dst/{year}/{month}/photo.bin。
    let dst_loc = smb_loc("dst/2024/01/photo.bin");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.inject_error(dst_loc, FakeOp::OpenWrite, std::io::ErrorKind::Other);

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    let result = tidy_with(
        &factory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: smb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("move should return Ok even with per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    // 精确计数：`>= 1` 杀不掉「+= 变 -=」变异（usize release 下 wrap 成 MAX 仍 >= 1）
    assert_eq!(
        report.failed, 1,
        "OpenWrite failure must be counted in failed exactly once: {report:?}"
    );
    // scanned = 0+0+failed：杀 make_report `+ failed` 变 `- failed`（wrap 成巨数）
    assert_eq!(
        report.scanned, 1,
        "single failing file must scan as 1: {report:?}"
    );
    assert!(src_file.exists(), "src must be kept on copy failure");
}

// 跨 backend move：copy 成功但源端 RemoveFile Err → src 保留 + dst 完整。
#[test]
fn move_keeps_src_and_dst_when_remove_file_fails() {
    let smb_src_root = smb_loc("src");
    let smb_src_file = smb_loc("src/photo.bin");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_src_root.clone());
    fake_smb.add_file(smb_src_file.clone(), vec![0xBB; 4096]);
    fake_smb.inject_error(
        smb_src_file.clone(),
        FakeOp::RemoveFile,
        std::io::ErrorKind::PermissionDenied,
    );

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", Arc::clone(&fake_smb) as Arc<dyn Backend>);

    let result = tidy_with(
        &factory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![smb_src_root],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("move should return Ok");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert_eq!(
        report.failed, 1,
        "remove_file failure must be counted in failed exactly once: {report:?}"
    );
    assert!(
        fake_smb.read_bytes(&smb_src_file).is_some(),
        "src must be kept on remove_file failure"
    );
    // FakeBackend metadata 走 UNIX_EPOCH → 1970/01 桶。
    let dst = out_dir.path().join("1970").join("01").join("photo.bin");
    assert!(dst.exists(), "dst must hold the completed copy: {dst:?}");
}

// 故障恢复后重跑：第一次 OpenWrite Err → src 保留；构造无注入的新 backend 再跑 →
// src 移走、dst 完整。模拟手册 §B3 "清半文件 → 幂等重跑" 流程。
#[test]
fn move_retry_after_target_open_write_failure_succeeds() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, vec![0xCC; 4096]).unwrap();
    fix_mtime(&src_file);

    let smb_root = smb_loc("dst");
    let dst_loc = smb_loc("dst/2024/01/photo.bin");

    // Round 1：注入 OpenWrite Err
    let fake1 = Arc::new(FakeBackend::new("smb"));
    fake1.add_dir(smb_root.clone());
    fake1.inject_error(
        dst_loc.clone(),
        FakeOp::OpenWrite,
        std::io::ErrorKind::Other,
    );

    let mut factory1 = FakeBackendFactory::new();
    factory1.insert("smb", fake1);

    let r1 = tidy_with(
        &factory1,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: smb_root.clone(),
            archive_template: None,
            report: None,
        },
    )
    .expect("round 1");
    let CommandResult::Copy(rep1) = r1 else {
        panic!("expected Copy report");
    };
    assert_eq!(rep1.failed, 1);
    assert!(src_file.exists(), "round 1: src must be kept");

    // Round 2：全新 backend，无注入，模拟环境恢复
    let fake2 = Arc::new(FakeBackend::new("smb"));
    fake2.add_dir(smb_root.clone());

    let mut factory2 = FakeBackendFactory::new();
    factory2.insert("smb", Arc::clone(&fake2) as Arc<dyn Backend>);

    let r2 = tidy_with(
        &factory2,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: smb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("round 2");
    let CommandResult::Copy(rep2) = r2 else {
        panic!("expected Copy report");
    };
    assert_eq!(rep2.failed, 0, "round 2 must succeed: {rep2:?}");
    assert!(rep2.copied >= 1);
    assert!(!src_file.exists(), "round 2: src must be moved");
    assert!(
        fake2.read_bytes(&dst_loc).is_some(),
        "round 2: dst must hold completed copy"
    );
}

// 跨 backend move：目标 OpenWrite 成功后 writer.write Err → stream_copy 内
// `if let Err(e) = result` True arm 触发，cleanup 调 remove_file 清半截目标。
// 区别于 `move_keeps_src_when_target_open_write_fails`：那个让 `open_write` 早返
// （ops.rs L120 `?`），未进入 `std::io::copy`；此测试让 write 阶段 Err，达 L122。
#[test]
fn move_keeps_src_when_target_write_fails() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, vec![0xCC; 4096]).unwrap();
    fix_mtime(&src_file);

    let smb_root = smb_loc("dst");
    let dst_loc = smb_loc("dst/2024/01/photo.bin");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.inject_writer_error(dst_loc.clone(), std::io::ErrorKind::BrokenPipe);

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", Arc::clone(&fake_smb) as Arc<dyn Backend>);

    let result = tidy_with(
        &factory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: smb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("move should return Ok even with per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert_eq!(
        report.failed, 1,
        "write failure must be counted in failed exactly once: {report:?}"
    );
    assert!(src_file.exists(), "src must be kept on stream-copy failure");
    // stream_copy cleanup 必须 remove_file 清半截目标。
    assert!(
        fake_smb.read_bytes(&dst_loc).is_none(),
        "partial dst must be cleaned up after write failure"
    );
}

// 跨 backend copy 成功后 `Info::open(target, output_backend)?` Err arm：
// fake_smb 注入 target 路径 OpenRead Err，stream_copy 先写入完成 → Info::open 失败。
// 本测试在集成 binary 触发 ops.rs L103 `?` Err region（lib unit 已有同语义测试，
// 缺集成 instance 让 region phantom 在集成 codegen 副本也 hit）。
#[test]
fn copy_reports_failure_when_info_open_after_transfer_fails() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("photo.bin");
    std::fs::write(&src_file, vec![0xDD; 4096]).unwrap();
    fix_mtime(&src_file);

    let smb_root = smb_loc("dst");
    let dst_loc = smb_loc("dst/2024/01/photo.bin");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    // stream_copy 先调 out_be.open_write 成功 → FakeWriter.finish 写入 → 然后
    // do_copy 末尾 Info::open 经 backend.open_read(target) 时注入 Err。
    fake_smb.inject_error(dst_loc, FakeOp::OpenRead, std::io::ErrorKind::Interrupted);

    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", Arc::clone(&fake_smb) as Arc<dyn Backend>);

    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: smb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("copy returns Ok with per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    assert_eq!(
        report.failed, 1,
        "Info::open after-transfer failure must be counted as one failure: {report:?}"
    );
}

// generate_unique_name 耗尽：output 子桶预先塞满同名 + 9 个 _i 变体（max_attempts=10），
// do_copy 内 `if let Some(..) = generate_unique_name(..)?` 走 None 分支 → ops.rs L106
// Err arm 触发。本测试在集成 binary 触发该路径（lib unit 已有同语义测试，缺集成 instance）。
#[test]
fn copy_reports_failure_when_unique_name_exhausted() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("sample-with-offset.jpg");
    std::fs::copy(format!("{DATA_DIR}/sample-with-offset.jpg"), &src_file)
        .expect("copy fixture to tempdir");

    // sample-with-offset.jpg EXIF DateTimeOriginal=2024:05:01 → 归档桶 2024/05。
    let out = tempdir().unwrap();
    let sub = out.path().join("2024").join("05");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("sample-with-offset.jpg"), b"").unwrap();
    for i in 1..10 {
        std::fs::write(sub.join(format!("sample-with-offset_{i}.jpg")), b"").unwrap();
    }

    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy returns Ok even when per-file unique name exhausts");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    assert_eq!(
        report.failed, 1,
        "exhausted unique-name target must count as one failure: {report:?}"
    );
}
