use chrono::TimeZone;
use chrono::Utc;

use super::build_entry;
use crate::entities::media_time::{
    Confidence, Conflict, ConflictKind, MediaTimeDecision, Priority, Source,
};

fn decision() -> MediaTimeDecision {
    MediaTimeDecision {
        utc: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        offset: Some(crate::usecases::config::chrono_offset_from_hours(8)),
        priority: Priority::P2,
        source: Source::FilenamePhone,
        inferred_offset: true,
        confidence: Confidence::High,
        conflicts: vec![Conflict {
            kind: ConflictKind::FilenameOver1Day,
            other_utc: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            other_source: Some(Source::FsMtime),
            diff_secs: 2 * 86_400,
        }],
    }
}

#[test]
fn build_entry_without_decision_marks_none() {
    let e = build_entry(
        "/tmp/a.jpg",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        "not_checked".to_owned(),
        Vec::new(),
        None,
    );
    assert_eq!(e.source_path, "/tmp/a.jpg");
    assert_eq!(e.chosen_priority, "(none)");
    assert_eq!(e.chosen_source, "(none)");
    assert!(e.conflicts.is_empty());
    assert!(e.actual_bucket.is_empty());
    assert!(e.exif_exp_bucket.is_none());
    assert!(e.filename_bucket.is_none());
    assert_eq!(e.duplicate_verdict, "not_checked");
    assert!(e.patterns.is_empty());
    assert!(e.fix_suggestion.is_none());
    assert!(!e.mismatch);
}

#[test]
fn build_entry_with_decision_fills_priority_source_conflicts() {
    let d = decision();
    let e = build_entry(
        "/tmp/IMG_20240101_120000.jpg",
        Some(&d),
        Some("2024:01".to_owned()),
        Some("2023:06".to_owned()),
        true,
        Some("DTO".to_owned()),
        Some("Panasonic".to_owned()),
        Some("DMC-GF6".to_owned()),
        Some("2024:01".to_owned()),
        "exact_dup".to_owned(),
        vec!["ExactDuplicate".to_owned()],
        Some("exiftool hint".to_owned()),
    );
    assert_eq!(e.chosen_priority, "P2");
    assert_eq!(e.chosen_source, "FilenamePhone");
    assert_eq!(e.conflicts, vec!["FilenameOver1Day"]);
    assert_eq!(e.actual_bucket, "2024:01");
    assert_eq!(e.exif_exp_bucket.as_deref(), Some("2023:06"));
    assert_eq!(e.exif_from.as_deref(), Some("DTO"));
    assert_eq!(e.exif_make.as_deref(), Some("Panasonic"));
    assert_eq!(e.exif_model.as_deref(), Some("DMC-GF6"));
    assert_eq!(e.filename_bucket.as_deref(), Some("2024:01"));
    assert_eq!(e.duplicate_verdict, "exact_dup");
    assert_eq!(e.patterns, vec!["ExactDuplicate"]);
    assert_eq!(e.fix_suggestion.as_deref(), Some("exiftool hint"));
    assert!(e.mismatch);
}

use std::path::Path;

use super::super::verify::exif_tsv::ExifRow;
use super::{load_tsv, lookup_tsv, strip_source_root};

#[test]
fn load_tsv_none_returns_empty() {
    assert!(load_tsv(None).is_empty());
}

#[test]
fn load_tsv_missing_file_returns_empty() {
    let p = Path::new("/definitely/missing/tsv.txt");
    assert!(load_tsv(Some(p)).is_empty());
}

#[test]
fn lookup_tsv_matches_normalized_path() {
    let row = ExifRow {
        path: "/out/IMG_1.jpg".to_owned(),
        fields: Default::default(),
        make: String::new(),
        model: String::new(),
    };
    let rows = vec![row];
    // 反斜杠路径归一化后匹配
    assert!(lookup_tsv(&rows, r"\out\IMG_1.jpg").is_some());
    assert!(lookup_tsv(&rows, "/other.jpg").is_none());
}

#[test]
fn strip_source_root_strips_root_prefix() {
    let roots = vec!["/photos".to_owned(), "/media".to_owned()];
    assert_eq!(
        strip_source_root("/photos/2024/01/a.jpg", &roots),
        "2024/01/a.jpg"
    );
    // 剥不掉时回退 basename
    assert_eq!(strip_source_root("/unrelated/x.jpg", &roots), "x.jpg");
    // 空剩余（根就是文件本身）回退 basename
    assert_eq!(strip_source_root("/photos", &roots), "photos");
}

#[test]
fn verify_collects_conflicts_from_media_time_decision() {
    crate::install_config_loader();
    let src = tempfile::tempdir().unwrap();
    let dst = src.path().join("IMG_20100101_120000.jpg");
    std::fs::copy("tests/data/sample-with-exif.jpg", &dst).unwrap();
    let ts = filetime::FileTime::from_unix_time(crate::entities::test_common::FIXED_MEDIA_MTIME, 0);
    filetime::set_file_mtime(&dst, ts).unwrap();
    let out = tempfile::tempdir().unwrap();
    let be: std::sync::Arc<dyn crate::entities::backend::Backend> =
        crate::adapters::backend::local::LocalBackend::arc();
    let loc = |p: &std::path::Path| {
        crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(p.to_str().unwrap()))
    };
    let report = super::verify(
        &[(loc(src.path()), be.clone())],
        &(loc(out.path()), be),
        super::DEFAULT_PHASH_MAX,
        false,
        None,
    );
    assert_eq!(report.entries.len(), 1);
    let conflicts = &report.entries[0].conflicts;
    assert!(conflicts.iter().any(|c| c == "FilenameOver1Day"));
}

#[test]
fn load_tsv_reads_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("exif.tsv");
    std::fs::write(&p, "/a/IMG_1.jpg\t2024:01:01 12:00:00\t-\t-\t-\t-\t-\t-\n").unwrap();
    assert_eq!(load_tsv(Some(&p)).len(), 1);
}

#[test]
fn strip_source_root_without_file_name_returns_input() {
    let roots = vec!["/photos".to_owned()];
    assert_eq!(strip_source_root("..", &roots), "..");
}
