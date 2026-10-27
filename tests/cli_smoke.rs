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

#[test]
fn cli_verify_reads_exif_tsv_from_stdin() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let report_dir = tempfile::tempdir().unwrap();
    let jpg = src.path().join("IMG_20240101_120000.jpg");
    std::fs::write(
        &jpg,
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    let report = report_dir.path().join("verify.json");
    let tsv = format!(
        "{}\t2023:06:01 10:00:00\t-\t-\t-\t-\tPanasonic\tDMC-GF6\n",
        jpg.display()
    );
    tidymedia()
        .arg("verify")
        .arg(src.path())
        .arg("--output")
        .arg(out.path())
        .arg("--exif-tsv")
        .arg("-")
        .arg("--report")
        .arg(&report)
        .write_stdin(tsv)
        .assert()
        .failure();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(json["mismatched"], 1);
    assert_eq!(json["entries"][0]["exif_exp_bucket"], "2023:06");
}

#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;
