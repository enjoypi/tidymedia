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
