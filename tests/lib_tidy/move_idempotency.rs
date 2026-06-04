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
