use std::path::Path;

use tempfile::tempdir;
use tidymedia::run_cli;

use super::DATA_DIR;

const EXIF_FIXTURE: &str = "sample-with-offset.jpg";

pub(super) fn seed_fixture(dir: &Path) -> std::path::PathBuf {
    let dst = dir.join(EXIF_FIXTURE);
    std::fs::copy(format!("{DATA_DIR}/{EXIF_FIXTURE}"), &dst).expect("seed fixture into tempdir");
    dst
}

fn file_count_recursive(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                file_count_recursive(&p)
            } else {
                1
            }
        })
        .sum()
}

#[test]
fn run_cli_copy_real_run_writes_output() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy without --dry-run via run_cli should succeed");
    assert!(
        file_count_recursive(out.path()) > 0,
        "real copy must write at least one file under output"
    );
}

#[test]
fn run_cli_move_real_run_removes_source_and_writes_output() {
    let src = tempdir().unwrap();
    let src_file = seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move without --dry-run via run_cli should succeed");
    assert!(!src_file.exists(), "real move must remove the source file");
    assert!(
        file_count_recursive(out.path()) > 0,
        "real move must write at least one file under output"
    );
}

#[test]
fn run_cli_copy_archive_template_writes_year_month_day_layout() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{year}/{month}/{day}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy --archive-template via run_cli should succeed");
    let day_dir = out.path().join("2024").join("05").join("01");
    assert!(
        file_count_recursive(&day_dir) > 0,
        "expected file under {}",
        day_dir.display()
    );
}

#[test]
fn run_cli_copy_rejects_invalid_archive_template() {
    let out = tempdir().unwrap();
    let err = run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{year/{month}", // unbalanced brace
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

#[test]
fn run_cli_move_archive_template_writes_year_month_layout() {
    let src = tempdir().unwrap();
    let src_file = seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--archive-template",
        "{year}/{month}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --archive-template via run_cli should succeed");
    assert!(!src_file.exists(), "move must remove the source file");
    let month_dir = out.path().join("2024").join("05");
    assert!(
        file_count_recursive(&month_dir) > 0,
        "expected file under {}",
        month_dir.display()
    );
}

#[test]
fn run_cli_move_rejects_invalid_archive_template() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let err = run_cli([
        "tidymedia",
        "move",
        "--archive-template",
        "year}", // extra closing brace
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

#[test]
fn run_cli_copy_report_writes_json() {
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("copy_report.json");
    run_cli([
        "tidymedia",
        "copy",
        "--dry-run",
        "--report",
        report_path.to_str().unwrap(),
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("copy --report via run_cli should succeed");
    let content = std::fs::read_to_string(&report_path).expect("report file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("report must be JSON");
    assert!(parsed["scanned"].as_u64().is_some());
}

#[test]
fn run_cli_move_report_writes_json() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("move_report.json");
    run_cli([
        "tidymedia",
        "move",
        "--dry-run",
        "--report",
        report_path.to_str().unwrap(),
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --report via run_cli should succeed");
    assert!(report_path.exists(), "report should be written");
}

#[test]
fn run_cli_find_report_writes_json() {
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("find_report.json");
    run_cli([
        "tidymedia",
        "find",
        "--report",
        report_path.to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("find --report via run_cli should succeed");
    let content = std::fs::read_to_string(&report_path).expect("report file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("report must be JSON");
    assert!(parsed["scanned"].as_u64().is_some());
}

#[test]
fn run_cli_find_with_output_dispatches() {
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "find",
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("find --output via run_cli should succeed");
}

#[test]
fn run_cli_copy_missing_required_output_returns_err() {
    let err = run_cli(["tidymedia", "copy", DATA_DIR]).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("--output"), "got: {msg}");
}

#[test]
fn run_cli_copy_archive_template_renders_make_model_from_exif() {
    let src = tempdir().unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-make-model.jpg"),
        src.path().join("m.jpg"),
    )
    .expect("seed make/model fixture");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{make}/{model}",
        "-o",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy with make/model template should succeed");
    assert!(
        out.path().join("TestCam/TestModel/m.jpg").exists(),
        "make/model from real EXIF must shape the archive dir"
    );
}

#[test]
fn run_cli_copy_avi_archives_by_embedded_exif() {
    let src = tempdir().unwrap();
    let avi = src.path().join("sample-fuji-strd.avi");
    std::fs::copy(format!("{DATA_DIR}/sample-fuji-strd.avi"), &avi).expect("seed AVI fixture");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy AVI via run_cli should succeed");
    assert!(
        out.path()
            .join("2005")
            .join("04")
            .join("sample-fuji-strd.avi")
            .exists(),
        "AVI must be archived by embedded EXIF time 2005/04"
    );
}
