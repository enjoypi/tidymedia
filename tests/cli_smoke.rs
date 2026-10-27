use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

fn tidymedia() -> Command {
    cargo_bin_cmd!("tidymedia")
}

#[test]
fn cli_help_exits_zero() {
    tidymedia()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage"));
}

#[test]
fn cli_cull_subcommand_is_removed() {
    tidymedia()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("cull").not());
    tidymedia().args(["cull", "--help"]).assert().failure();
}

#[test]
fn cli_version_exits_zero() {
    tidymedia().arg("--version").assert().success();
}

#[test]
fn cli_find_on_tests_data_succeeds() {
    tidymedia().args(["find", "tests/data"]).assert().success();
}

#[test]
fn cli_copy_help_only_lists_local_and_smb_sources() {
    tidymedia()
        .args(["copy", "--help"])
        .assert()
        .success()
        .stdout(contains("smb://"))
        .stdout(contains("adb://").not())
        .stdout(contains("mtp://").not());
}

#[test]
fn cli_unknown_subcommand_fails() {
    tidymedia()
        .arg("definitely-not-a-subcommand")
        .assert()
        .failure();
}

#[test]
fn cli_report_write_failures_exit_nonzero() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let report = "/nonexistent_dir_xyz/report.json";
    let src = src.path().to_str().unwrap();
    let out = out.path().to_str().unwrap();
    tidymedia()
        .args(["find", src, "-o", out, "--report", report])
        .assert()
        .failure();
    tidymedia()
        .args([
            "move-text-shot",
            "--dry-run",
            src,
            "-o",
            out,
            "--report",
            report,
        ])
        .env("TIDYMEDIA_OCR_DET_MODEL", "unused.onnx")
        .assert()
        .failure();
    tidymedia()
        .args(["verify", src, "-o", out, "--report", report])
        .assert()
        .failure();
}

#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;
