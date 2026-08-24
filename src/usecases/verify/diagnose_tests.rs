use crate::entities::media_time::ConflictKind;
use crate::usecases::verify::diagnose::{DiagnoseInput, fix_suggestion, patterns};

fn input<'a>(
    actual: Option<&'a str>,
    exp: Option<&'a str>,
    from: Option<&'a str>,
    name: Option<&'a str>,
    conflicts: &'a [ConflictKind],
    verdict: &'a str,
    mismatch: bool,
) -> DiagnoseInput<'a> {
    DiagnoseInput {
        actual_bucket: actual,
        exp_bucket: exp,
        exif_from: from,
        filename_bucket: name,
        conflicts,
        duplicate_verdict: verdict,
        mismatch,
    }
}

#[test]
fn container_miss_requires_qt_from_and_mismatch() {
    let p = patterns(&input(
        Some("2020:07"),
        Some("2020:08"),
        Some("QTCreationDate"),
        None,
        &[],
        "absent",
        true,
    ));
    assert!(p.contains(&"TidymediaContainerMiss".to_owned()), "{p:?}");
}

#[test]
fn dto_mismatch_is_not_container_miss() {
    let p = patterns(&input(
        Some("2024:01"),
        Some("2023:06"),
        Some("DTO"),
        None,
        &[],
        "absent",
        true,
    ));
    assert!(!p.contains(&"TidymediaContainerMiss".to_owned()));
}

#[test]
fn camera_clock_unset_on_zero_bucket() {
    let p = patterns(&input(
        None,
        Some("0000:00"),
        Some("QTCreationDate"),
        None,
        &[],
        "absent",
        true,
    ));
    assert!(p.contains(&"CameraClockUnset".to_owned()));
}

#[test]
fn fs_time_is_copy_stamp_on_mtime_conflict() {
    let p = patterns(&input(
        Some("2020:07"),
        None,
        None,
        None,
        &[ConflictKind::MtimeMuchEarlierThanP0],
        "absent",
        false,
    ));
    assert!(p.contains(&"FsTimeIsCopyStamp".to_owned()));
}

#[test]
fn filename_differs_when_bucket_conflicts() {
    let p = patterns(&input(
        Some("2024:01"),
        None,
        None,
        Some("2019:03"),
        &[],
        "absent",
        false,
    ));
    assert!(p.contains(&"FilenameDateDiffers".to_owned()));
}

#[test]
fn exact_duplicate_pattern_on_verdict() {
    let p = patterns(&input(None, None, None, None, &[], "exact_dup", false));
    assert!(p.contains(&"ExactDuplicate".to_owned()));
}

#[test]
fn no_patterns_on_clean_entry() {
    let p = patterns(&input(
        Some("2024:01"),
        Some("2024:01"),
        Some("DTO"),
        Some("2024:01"),
        &[],
        "absent",
        false,
    ));
    assert!(p.is_empty(), "{p:?}");
}

#[test]
fn exact_dup_needs_no_fix_suggestion() {
    let s = fix_suggestion(&input(None, None, None, None, &[], "exact_dup", false));
    assert!(s.is_none());
}

#[test]
fn mismatch_yields_fix_suggestion() {
    let s = fix_suggestion(&input(
        Some("2024:01"),
        Some("2023:06"),
        Some("DTO"),
        None,
        &[],
        "absent",
        true,
    ));
    let text = s.expect("mismatch should yield suggestion");
    assert!(text.contains("exiftool"), "{text}");
}

#[test]
fn clean_entry_has_no_fix_suggestion() {
    let s = fix_suggestion(&input(
        Some("2024:01"),
        Some("2024:01"),
        Some("DTO"),
        Some("2024:01"),
        &[],
        "absent",
        false,
    ));
    assert!(s.is_none());
}
