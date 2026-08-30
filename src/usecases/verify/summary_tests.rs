use std::collections::BTreeMap;

use super::format_summary;
use crate::usecases::verify::report::{VerifyEntry, VerifyReport};

fn entry(source: &str) -> VerifyEntry {
    VerifyEntry {
        source_path: source.to_owned(),
        actual_bucket: "2024:01".to_owned(),
        duplicate_verdict: "absent".to_owned(),
        ..Default::default()
    }
}

#[test]
fn empty_report_prints_zero_sections_only() {
    let out = format_summary(&VerifyReport::default());
    assert!(out.contains("scanned=0"));
    assert!(out.contains("MISMATCH_count=0"));
    assert!(out.contains("DIFFER_count=0"));
    assert!(out.contains("with_name_time=0"));
    assert!(!out.contains("---mismatch by from---"));
    assert!(!out.contains("---MISMATCH details---"));
    assert!(!out.contains("---DIFFER details---"));
    assert!(!out.contains("---patterns---"));
    assert!(out.contains("---duplicate_verdict---\n"));
}

#[test]
fn mismatch_rows_group_by_from_with_fallbacks() {
    let mut a = entry("/src/a.jpg");
    a.mismatch = true;
    a.exif_exp_bucket = Some("2023:12".to_owned());
    a.exif_from = Some("DTO".to_owned());
    a.exif_make = Some("Canon".to_owned());
    a.exif_model = Some(String::new());
    let mut b = entry("/src/b.mov");
    b.mismatch = true;
    let report = VerifyReport {
        entries: vec![a, b],
        ..Default::default()
    };
    let out = format_summary(&report);
    assert!(out.contains("MISMATCH_count=2"));
    assert!(out.contains("---mismatch by from---\n1\tDTO\n1\tNONE\n"));
    assert!(
        out.contains(
            "MISMATCH\texp=2023:12\ttgt=2024:01\tfrom=DTO\tmake=Canon\tmodel=-\t/src/a.jpg"
        )
    );
    assert!(
        out.contains("MISMATCH\texp=NONE\ttgt=2024:01\tfrom=NONE\tmake=-\tmodel=-\t/src/b.mov")
    );
}

#[test]
fn differ_only_when_filename_bucket_differs() {
    let mut same = entry("/src/same.jpg");
    same.filename_bucket = Some("2024:01".to_owned());
    let mut diff = entry("/src/diff.jpg");
    diff.filename_bucket = Some("2019:05".to_owned());
    let report = VerifyReport {
        entries: vec![same, diff],
        ..Default::default()
    };
    let out = format_summary(&report);
    assert!(out.contains("with_name_time=2"));
    assert!(out.contains("DIFFER_count=1"));
    assert!(out.contains("DIFFER\tname=2019:05\ttgt=2024:01\t/src/diff.jpg"));
}

#[test]
fn verdict_counts_sort_by_count_then_key() {
    let mut e1 = entry("/src/1.jpg");
    e1.duplicate_verdict = "absent".to_owned();
    let mut e2 = entry("/src/2.jpg");
    e2.duplicate_verdict = "name_only".to_owned();
    let mut e3 = entry("/src/3.jpg");
    e3.duplicate_verdict = "exact_dup".to_owned();
    let report = VerifyReport {
        entries: vec![e1, e2, e3, entry("/src/4.jpg")],
        ..Default::default()
    };
    let out = format_summary(&report);
    let section = out.split("---duplicate_verdict---\n").nth(1).unwrap_or("");
    assert_eq!(section, "2\tabsent\n1\texact_dup\n1\tname_only\n");
}

#[test]
fn pattern_counts_sort_by_count_then_key() {
    let mut counts = BTreeMap::new();
    counts.insert("FilenameDateDiffers".to_owned(), 1_usize);
    counts.insert("ExactDuplicate".to_owned(), 2_usize);
    counts.insert("CameraClockUnset".to_owned(), 1_usize);
    let report = VerifyReport {
        pattern_counts: counts,
        ..Default::default()
    };
    let out = format_summary(&report);
    let section = out.split("---patterns---\n").nth(1).unwrap_or("");
    assert_eq!(
        section,
        "2\tExactDuplicate\n1\tCameraClockUnset\n1\tFilenameDateDiffers\n"
    );
}
