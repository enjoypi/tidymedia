// spec §七：判定结果应包含的字段。

use tidymedia::media_time::{epoch_to_candidate, resolve, Confidence, ConflictKind, Priority, Source};

use super::common::{fixed_now, ts, utc_offset};

/// spec §7：MediaTimeDecision 包含 utc / offset / priority / source / inferred_offset
/// / confidence / conflicts 七个字段。
#[test]
fn decision_carries_all_seven_fields() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        true, // inferred
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 48 * 3600);
    let d = resolve(vec![p0], Some(gps), fixed_now()).unwrap();

    // utc
    assert_eq!(d.utc.timestamp(), 1_700_000_100);
    // offset
    assert_eq!(d.offset, Some(utc_offset()));
    // priority
    assert_eq!(d.priority, Priority::P0);
    // source
    assert_eq!(d.source, Source::ExifDateTimeOriginal);
    // inferred_offset
    assert!(d.inferred_offset);
    // confidence
    assert_eq!(d.confidence, Confidence::High);
    // conflicts (含 GPS 超过 24h)
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
    assert_eq!(d.conflicts[0].other_utc.timestamp(), 1_700_000_100 + 48 * 3600);
}

/// spec §7：confidence 字段反映置信度（High / Low），用于"是否需要人工复核"。
#[test]
fn confidence_signals_pre_1995_for_human_review() {
    let pre = epoch_to_candidate(315_532_800, Source::ExifDateTimeOriginal, None, false).unwrap();
    let d = resolve(vec![pre], None, fixed_now()).unwrap();
    assert_eq!(d.confidence, Confidence::Low);
}

/// spec §7：conflicts 可以包含多条（GPS + filename 同时冲突）。
#[test]
fn conflicts_can_carry_multiple_entries() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let fname = epoch_to_candidate(
        1_700_000_100 + 5 * 86_400,
        Source::FilenamePhone,
        Some(utc_offset()),
        true,
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 48 * 3600);
    let d = resolve(vec![p0, fname], Some(gps), fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 2);
}

/// spec §7：other_source 字段标记冲突来源（用于人工复核理解上下文）。
#[test]
fn conflict_carries_other_source_when_internal() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let fname = epoch_to_candidate(
        1_700_000_100 + 5 * 86_400,
        Source::FilenameUnixMillis,
        None,
        false,
    )
    .unwrap();
    let d = resolve(vec![p0, fname], None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].other_source, Some(Source::FilenameUnixMillis));
}

/// spec §7：GPS 冲突无 internal Source（GPS 不在 P0-P4 体系内）。
#[test]
fn gps_conflict_has_no_other_source() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 48 * 3600);
    let d = resolve(vec![p0], Some(gps), fixed_now()).unwrap();
    assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
    assert_eq!(d.conflicts[0].other_source, None);
}
