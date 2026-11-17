use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;
use crate::usecases::memory::with_retained;
use crate::usecases::verify::manifest::with_manifest_min_entries;
use crate::usecases::verify::summary::format_summary;
use crate::usecases::verify::{DEFAULT_PHASH_MAX, VerifyReport, verify};

const FIXTURES: [&str; 6] = [
    "sample-with-exif.jpg",
    "sample-png-exif.png",
    "sample-with-gps.jpg",
    "sample-only-createdate.jpg",
    "sample-with-make-model.jpg",
    "sample-with-offset.jpg",
];

fn seed(src: &Path, out: &Path) {
    for name in FIXTURES {
        fs::copy(Path::new("tests/data").join(name), src.join(name)).unwrap();
    }
    fs::create_dir_all(out.join("2024")).unwrap();
    fs::copy("tests/data/sample-with-exif.jpg", out.join("2024/kept.jpg")).unwrap();
    fs::copy(
        "tests/data/sample-no-dates.jpg",
        out.join("2024/sample-with-gps.jpg"),
    )
    .unwrap();
    fs::copy(
        "tests/data/sample-pre-1995.jpg",
        out.join("2024/sample-with-offset_1.jpg"),
    )
    .unwrap();
}

fn loc(path: &Path) -> Location {
    Location::Local(Utf8PathBuf::from(path.to_str().unwrap()))
}

fn run(capacity: u64, src: &Path, out: &Path) -> VerifyReport {
    let be: std::sync::Arc<dyn Backend> = LocalBackend::arc();
    with_retained(capacity, || {
        verify(
            &[(loc(src), be.clone())],
            &(loc(out), be.clone()),
            DEFAULT_PHASH_MAX,
            false,
            None,
        )
        .unwrap()
    })
}

fn json(report: &VerifyReport) -> serde_json::Value {
    let mut value = serde_json::to_value(report).unwrap();
    value["duration_ms"] = serde_json::Value::Null;
    value
}

#[test]
fn grouped_verify_matches_single_pass_report_and_summary() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let single = run(1 << 30, src.path(), out.path());
    let grouped = run(16_000, src.path(), out.path());
    assert!(!single.entries.is_spilled());
    assert!(grouped.entries.is_spilled());
    assert_eq!(json(&grouped), json(&single));
    assert_eq!(format_summary(&grouped), format_summary(&single));
    assert_eq!(single.entries.len(), FIXTURES.len());
    let verdicts: Vec<&str> = single
        .entries
        .iter()
        .map(|e| e.duplicate_verdict.as_str())
        .collect();
    assert!(verdicts.contains(&"exact_dup"));
    assert!(
        verdicts.iter().filter(|v| **v != "absent").count() >= 3,
        "{verdicts:?}"
    );
}

#[test]
fn identical_flood_is_chunked_and_matches_single_pass() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    for i in 0..6 {
        fs::copy(
            "tests/data/sample-with-exif.jpg",
            src.path().join(format!("{i}.jpg")),
        )
        .unwrap();
    }
    fs::copy("tests/data/sample-with-exif.jpg", out.path().join("0.jpg")).unwrap();
    let single = run(1 << 30, src.path(), out.path());
    let chunked = run(12_000, src.path(), out.path());
    assert!(chunked.entries.is_spilled());
    assert_eq!(json(&chunked), json(&single));
}

#[test]
fn entry_larger_than_budget_is_reported() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    fs::copy("tests/data/sample-with-exif.jpg", src.path().join("a.jpg")).unwrap();
    let be: std::sync::Arc<dyn Backend> = LocalBackend::arc();
    let err = with_retained(100, || {
        verify(
            &[(loc(src.path()), be.clone())],
            &(loc(out.path()), be.clone()),
            DEFAULT_PHASH_MAX,
            false,
            None,
        )
        .unwrap_err()
    });
    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), std::io::ErrorKind::OutOfMemory);
}

#[test]
fn grouped_verify_tolerates_missing_output_library() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let missing = out.path().join("missing");
    let single = run(1 << 30, src.path(), &missing);
    let grouped = run(16_000, src.path(), &missing);
    assert!(grouped.entries.is_spilled());
    assert_eq!(json(&grouped), json(&single));
}

#[test]
fn cached_grouping_matches_single_pass_report_and_summary() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let single = run(1 << 30, src.path(), out.path());
    let cached = with_manifest_min_entries(1, || run(16_000, src.path(), out.path()));
    assert!(cached.entries.is_spilled());
    assert_eq!(cached.scanned, single.scanned);
    assert_eq!(cached.compared, single.compared);
    assert_eq!(json(&cached), json(&single));
    assert_eq!(format_summary(&cached), format_summary(&single));
}

#[test]
fn cached_identical_flood_matches_single_pass() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    for i in 0..6 {
        fs::copy(
            "tests/data/sample-with-exif.jpg",
            src.path().join(format!("{i}.jpg")),
        )
        .unwrap();
    }
    fs::copy("tests/data/sample-with-exif.jpg", out.path().join("0.jpg")).unwrap();
    let single = run(1 << 30, src.path(), out.path());
    let cached = with_manifest_min_entries(1, || run(12_000, src.path(), out.path()));
    assert!(cached.entries.is_spilled());
    assert_eq!(json(&cached), json(&single));
}

#[test]
fn cached_grouping_tolerates_missing_output_library() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let missing = out.path().join("missing");
    let single = run(1 << 30, src.path(), &missing);
    let cached = with_manifest_min_entries(1, || run(16_000, src.path(), &missing));
    assert!(cached.entries.is_spilled());
    assert_eq!(json(&cached), json(&single));
}
