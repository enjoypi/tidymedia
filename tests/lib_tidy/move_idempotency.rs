//! Move 幂等性集成测试。验收手册 §B4 「同命令重跑期望 copied=0」对应契约。

use tempfile::tempdir;
use tidymedia::{CommandResult, Commands, tidy_with};

use super::{DATA_DIR, FakeBackendFactory, local};

fn copy_fixture(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let dst = dir.join(name);
    std::fs::copy(format!("{DATA_DIR}/{name}"), &dst).expect("copy fixture");
    dst
}

fn move_cmd(src: &std::path::Path, out: &std::path::Path, dry_run: bool) -> Commands {
    Commands::Move {
        dry_run,
        include_non_media: false,
        sources: vec![local(src.to_str().unwrap())],
        output: local(out.to_str().unwrap()),
        archive_template: None,
        report: None,
    }
}

// 同 src 跑 move 两次：第一次 copied≥1 src 清空；第二次 copied=0 / failed=0。
#[test]
fn move_local_to_local_second_run_is_noop() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let src_file = copy_fixture(src_dir.path(), "sample-with-offset.jpg");

    let factory = FakeBackendFactory::new();

    let r1 =
        tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), false)).expect("first move");
    let CommandResult::Copy(report1) = r1 else {
        panic!("expected Copy report, got {r1:?}");
    };
    assert!(
        report1.copied >= 1,
        "first move must copy at least 1: {report1:?}"
    );
    assert!(!src_file.exists(), "first move must remove src");

    let r2 =
        tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), false)).expect("second move");
    let CommandResult::Copy(report2) = r2 else {
        panic!("expected Copy report, got {r2:?}");
    };
    assert_eq!(report2.copied, 0, "second move must be no-op: {report2:?}");
    assert_eq!(report2.failed, 0);
}

// dry-run move：src 文件保留，out 目录无新增。
#[test]
fn move_dry_run_does_not_touch_src_or_output() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let src_file = copy_fixture(src_dir.path(), "sample-with-offset.jpg");

    let factory = FakeBackendFactory::new();
    let result =
        tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), true)).expect("dry-run move");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert!(report.dry_run, "dry_run flag must propagate to report");
    assert!(src_file.exists(), "src must remain after dry-run");
    assert!(
        std::fs::read_dir(out_dir.path()).unwrap().next().is_none(),
        "out must be empty after dry-run"
    );
}

// fast-path 命中：local→local + remove → src 删 / dst 落归档桶 / report.copied=1。
#[test]
fn move_local_fastpath_src_deleted_dst_exists() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let src_file = copy_fixture(src_dir.path(), "sample-with-offset.jpg");

    let factory = FakeBackendFactory::new();
    let r = tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), false)).expect("move");
    let CommandResult::Copy(report) = r else {
        panic!("expected Copy report, got {r:?}");
    };

    assert_eq!(report.copied, 1, "fast-path move must copy 1: {report:?}");
    assert!(!src_file.exists(), "src must be removed after move");
    let archived = out_dir
        .path()
        .join("2024")
        .join("05")
        .join("sample-with-offset.jpg");
    assert!(archived.exists(), "dst missing at {archived:?}");
}

// fast-path 走 fs::rename 不重写文件：dst mtime 必须等于 src 原 mtime
// （若走 stream_copy 路径 dst mtime = now，这里反向证明 fast-path 命中）。
#[test]
fn move_local_fastpath_dst_mtime_matches_src() {
    use std::time::SystemTime;

    const PINNED_SECS: u64 = 1_704_067_200; // 2024-01-01 UTC

    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let src_file = copy_fixture(src_dir.path(), "sample-with-offset.jpg");

    let pinned = filetime::FileTime::from_unix_time(PINNED_SECS.cast_signed(), 0);
    filetime::set_file_mtime(&src_file, pinned).expect("set src mtime");

    let factory = FakeBackendFactory::new();
    tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), false)).expect("move");

    let archived = out_dir
        .path()
        .join("2024")
        .join("05")
        .join("sample-with-offset.jpg");
    let dst_mtime = std::fs::metadata(&archived)
        .expect("dst exists")
        .modified()
        .expect("mtime");
    let dst_secs = dst_mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("mtime after epoch")
        .as_secs();
    assert_eq!(
        dst_secs, PINNED_SECS,
        "fast-path rename must preserve src mtime"
    );
}

// dry-run 之后接真跑：dry-run 不影响后续真跑的归档结果。
#[test]
fn move_dry_run_then_real_run_completes() {
    let src_dir = tempdir().unwrap();
    let out_dir = tempdir().unwrap();
    let src_file = copy_fixture(src_dir.path(), "sample-with-offset.jpg");

    let factory = FakeBackendFactory::new();

    let dry = tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), true)).expect("dry-run");
    let CommandResult::Copy(dry_report) = dry else {
        panic!("expected Copy report");
    };
    assert!(dry_report.dry_run);

    let real =
        tidy_with(&factory, move_cmd(src_dir.path(), out_dir.path(), false)).expect("real run");
    let CommandResult::Copy(real_report) = real else {
        panic!("expected Copy report");
    };
    assert!(!real_report.dry_run);
    assert!(
        real_report.copied >= 1,
        "real run must copy: {real_report:?}"
    );
    assert!(
        !src_file.exists(),
        "real run must remove src after dry-run preview"
    );
}

/// 源内两份相同内容 → 第二份按重复计入 ignored，且恰好一次。精确计数杀
/// `ignored += 1` 的 `-=`（usize wrap 成 MAX）与 `*=`（恒 0）算术变异。
#[test]
fn copy_counts_duplicate_source_as_ignored_exactly_once() {
    let src_dir = tempdir().unwrap();
    copy_fixture(src_dir.path(), "sample-with-offset.jpg");
    let dup = src_dir.path().join("dup-of-offset.jpg");
    std::fs::copy(format!("{DATA_DIR}/sample-with-offset.jpg"), &dup)
        .expect("seed duplicate fixture");
    let out_dir = tempdir().unwrap();

    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy with duplicate source should succeed");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    // scanned = copied+ignored+failed 的多项非零和：顺带杀 make_report 求和算术变异
    assert_eq!(
        (report.scanned, report.copied, report.ignored, report.failed),
        (2, 1, 1, 0),
        "second identical file must be ignored exactly once: {report:?}"
    );
}

/// `make_report` 的 scanned 求和（indexed + empty + unreadable + `walker_errors`）
/// 各项全非零 + 精确断言：任一 `+` 被变异成 `-`/`*`（含 u64/usize wrap）都会
/// 偏离期望值。copied=1 / empty=1 / unreadable=1 / walker=1 → scanned=4。
#[test]
#[cfg(unix)]
fn copy_report_scanned_sums_all_skip_categories_exactly() {
    use std::os::unix::fs::PermissionsExt;
    let src_dir = tempdir().unwrap();
    copy_fixture(src_dir.path(), "sample-with-offset.jpg");
    std::fs::write(src_dir.path().join("empty.bin"), b"").unwrap();
    let locked = src_dir.path().join("locked.bin");
    std::fs::write(&locked, b"abcdef").unwrap();
    let mut perms = std::fs::metadata(&locked).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&locked, perms.clone()).unwrap();
    let out_dir = tempdir().unwrap();

    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![
                local(src_dir.path().to_str().unwrap()),
                local("/no/such/dir/zzz_scanned_sum_xyz"), // walker_errors += 1
            ],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    );
    // 恢复权限以便 tempdir 清理
    perms.set_mode(0o644);
    std::fs::set_permissions(&locked, perms).unwrap();

    let CommandResult::Copy(report) = result.expect("copy should succeed") else {
        panic!("expected Copy report");
    };
    assert_eq!(
        (
            report.scanned,
            report.copied,
            report.skipped_empty,
            report.skipped_unreadable,
            report.walker_errors,
        ),
        (4, 1, 1, 1, 1),
        "scanned must be the exact sum of all categories: {report:?}"
    );
}

/// find 带合法 output 目录：必须真扫描（scanned >= 1）而非空 default 报告。
/// 杀 `m.kind == EntryKind::Dir` 被变异成 `!=`（合法目录被误判 → 短路返 default，
/// 现有 `run_cli` e2e 只断言 Ok 杀不掉）。
#[test]
fn find_with_valid_output_dir_scans_sources() {
    let out_dir = tempdir().unwrap();
    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Find {
            secure: false,
            sources: vec![local(DATA_DIR)],
            output: Some(local(out_dir.path().to_str().unwrap())),
            report: None,
        },
    )
    .expect("find with valid output dir should succeed");
    let CommandResult::Find(report) = result else {
        panic!("expected Find report");
    };
    assert!(
        report.scanned >= 1,
        "valid dir output must not short-circuit the scan: {report:?}"
    );
}
