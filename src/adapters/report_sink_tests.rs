use tempfile::tempdir;

use super::JsonFileReportSink;
use crate::usecases::report::{CopyReport, FindReport, Report};

fn sample_copy_report() -> CopyReport {
    CopyReport {
        scanned: 5,
        copied: 3,
        ignored: 1,
        failed: 1,
        skipped_empty: 0,
        skipped_unreadable: 0,
        walker_errors: 0,
        dry_run: false,
        remove: false,
        include_non_media: false,
        errors: vec![],
    }
}

fn sample_find_report() -> FindReport {
    FindReport {
        scanned: 10,
        groups: vec![vec!["a.jpg".into(), "a_copy.jpg".into()]],
        bytes_read: 1024,
    }
}

#[test]
fn sink_writes_valid_copy_json() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("report.json");
    let sink = JsonFileReportSink::new(path.to_str().unwrap());
    let r = sample_copy_report();
    sink.write(&Report::Copy(&r));
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["scanned"], 5);
    assert_eq!(parsed["copied"], 3);
    assert_eq!(parsed["dry_run"], false);
}

#[test]
fn sink_writes_valid_find_json() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("find_report.json");
    let sink = JsonFileReportSink::new(path.to_str().unwrap());
    let r = sample_find_report();
    sink.write(&Report::Find(&r));
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["scanned"], 10);
    assert_eq!(parsed["bytes_read"], 1024);
    let groups = parsed["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
}

#[test]
fn sink_copy_invalid_path_only_warns() {
    // 父目录不存在 → warn 不 panic
    let sink = JsonFileReportSink::new("/nonexistent_dir_xyz/r.json");
    let r = sample_copy_report();
    sink.write(&Report::Copy(&r));
}

#[test]
fn sink_find_invalid_path_only_warns() {
    let sink = JsonFileReportSink::new("/nonexistent_dir_xyz/r.json");
    let r = sample_find_report();
    sink.write(&Report::Find(&r));
}
