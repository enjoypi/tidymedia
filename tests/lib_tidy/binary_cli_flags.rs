use tempfile::tempdir;

use super::DATA_DIR;
use super::run_cli_flags::seed_fixture;

#[test]
fn binary_emits_debug_log_to_stderr_when_log_level_debug() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args(["--log-level", "debug", "find", src.path().to_str().unwrap()])
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn tidymedia binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stderr.contains("cli parsed"),
        "binary must log 'cli parsed' to stderr at debug level; status: {:?}, stderr: {stderr}",
        out.status
    );
}

const OFFICE_FIXTURES: &[&str] = &[
    "sample-pdf-dated.pdf",
    "sample-docx-dated.docx",
    "sample-pptx-dated.pptx",
    "sample-xlsx-dated.xlsx",
    "sample-doc-dated.doc",
    "sample-xls-dated.xls",
    "sample-ppt-dated.ppt",
    "sample-odt-dated.odt",
    "sample-ods-dated.ods",
    "sample-odp-dated.odp",
    "sample-rtf-dated.rtf",
    "sample-epub-dated.epub",
];

#[test]
fn binary_copy_with_office_fixtures_walks_office_paths() {
    let src = tempdir().unwrap();
    for name in OFFICE_FIXTURES {
        std::fs::copy(format!("{DATA_DIR}/{name}"), src.path().join(name))
            .unwrap_or_else(|e| panic!("seed {name}: {e}"));
    }
    let out = tempdir().unwrap();
    let outp = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args([
            "copy",
            "--include-non-media",
            "--dry-run",
            "--output",
            out.path().to_str().unwrap(),
            src.path().to_str().unwrap(),
        ])
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn tidymedia binary");
    assert!(
        outp.status.success(),
        "binary copy with office fixtures must succeed; stderr: {}",
        String::from_utf8_lossy(&outp.stderr)
    );
}

#[test]
fn binary_copy_doc_with_office_fixtures_walks_doc_paths() {
    let src = tempdir().unwrap();
    for name in OFFICE_FIXTURES {
        std::fs::copy(format!("{DATA_DIR}/{name}"), src.path().join(name))
            .unwrap_or_else(|e| panic!("seed {name}: {e}"));
    }
    let out = tempdir().unwrap();
    let outp = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args([
            "copy-doc",
            "--dry-run",
            "--archive-template",
            "{year}/{month}",
            "--output",
            out.path().to_str().unwrap(),
            src.path().to_str().unwrap(),
        ])
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn tidymedia binary");
    assert!(
        outp.status.success(),
        "binary copy-doc with office fixtures must succeed; stderr: {}",
        String::from_utf8_lossy(&outp.stderr)
    );
}

#[test]
fn binary_sanitizes_out_of_range_timezone_offset() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let cfg_dir = tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(&cfg_path, "copy:\n  timezone_offset_hours: 99\n").expect("write test config");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args(["find", src.path().to_str().unwrap()])
        .env_remove("RUST_LOG")
        .env("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap())
        .output()
        .expect("spawn tidymedia binary");
    assert!(
        out.status.success(),
        "binary must keep running with sanitized timezone; status: {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn binary_uses_config_log_level_when_flag_absent() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let cfg_dir = tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(&cfg_path, "log:\n  level: debug\n").expect("write test config");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args(["find", src.path().to_str().unwrap()])
        .env_remove("RUST_LOG")
        .env("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap())
        .output()
        .expect("spawn tidymedia binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stderr.contains("cli parsed"),
        "binary must honor config log.level=debug; status: {:?}, stderr: {stderr}",
        out.status
    );
}

#[test]
fn binary_find_with_output_emits_rm_for_outside_and_comment_for_inside() {
    let root = tempdir().unwrap();
    let keep = root.path().join("keep");
    std::fs::create_dir(&keep).unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-offset.jpg"),
        keep.join("a.jpg"),
    )
    .expect("seed kept copy");
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-offset.jpg"),
        root.path().join("b.jpg"),
    )
    .expect("seed duplicate outside output");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args([
            "find",
            root.path().to_str().unwrap(),
            "-o",
            keep.to_str().unwrap(),
        ])
        .output()
        .expect("spawn tidymedia binary");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let rm_outside = stdout
        .lines()
        .any(|l| l.starts_with("os.remove(") && l.contains("b.jpg"));
    let kept_commented = stdout
        .lines()
        .filter(|l| l.contains("a.jpg"))
        .all(|l| !l.starts_with("os.remove("));
    assert!(
        rm_outside && kept_commented,
        "outside dup must be rm'd, kept copy commented; stdout:\n{stdout}"
    );
}
