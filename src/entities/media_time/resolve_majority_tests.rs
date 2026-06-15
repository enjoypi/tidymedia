use chrono::TimeZone;
use chrono::Utc;

use crate::entities::media_time::decision::ConflictKind;
use crate::entities::media_time::priority::Priority;
use crate::entities::media_time::priority::Source;
use crate::entities::media_time::resolve::resolve;

use super::tests_common::cand;
use super::tests_common::now;

const CONFLICT_OVER_DAY_SECS: i64 = 86_400;

#[test]
fn majority_filename_mtime_overrules_wrong_p0() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.utc.timestamp(), real);
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.source, Source::FilenameDashedDateTime);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
    assert_eq!(d.conflicts[0].other_utc.timestamp(), p0);
    assert_eq!(
        d.conflicts[0].other_source,
        Some(Source::ExifDateTimeOriginal)
    );
}

#[test]
fn p0_kept_when_filename_lacks_mtime_corroboration() {
    let p0 = 1_000_000_000;
    let f = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, f),
            cand(Source::FsMtime, f + 3 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_no_mtime_candidate() {
    let p0 = 1_000_000_000;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, p0 + 600 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_filename_within_30_days() {
    let p0 = 1_700_000_100;
    let f = p0 + 10 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, f),
            cand(Source::FsMtime, f),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_modify_date_corroborates_majority() {
    let p0 = 1_000_000_000;
    let resave = p0 + 600 * 86_400;
    let md = Utc.timestamp_opt(resave + 1800, 0).single().unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, resave),
            cand(Source::FsMtime, resave + 3600),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts.len(), 2);
    assert_eq!(
        d.conflicts[0].kind,
        ConflictKind::MajorityVetoedByModifyDate
    );
    assert_eq!(d.conflicts[0].other_utc.timestamp(), resave);
    assert_eq!(
        d.conflicts[0].other_source,
        Some(Source::FilenameDashedDateTime)
    );
    assert_eq!(d.conflicts[0].diff_secs, p0 - resave);
    assert_eq!(d.conflicts[1].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn majority_overrules_when_modify_date_missing() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
}

#[test]
fn majority_overrules_when_modify_date_far_from_filename() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let md = Utc
        .timestamp_opt(real + CONFLICT_OVER_DAY_SECS + 1, 0)
        .single()
        .unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.utc.timestamp(), real);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
}

#[test]
fn modify_date_at_exactly_one_day_still_vetoes() {
    let p0 = 1_000_000_000;
    let resave = p0 + 600 * 86_400;
    let md = Utc
        .timestamp_opt(resave + CONFLICT_OVER_DAY_SECS, 0)
        .single()
        .unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, resave),
            cand(Source::FsMtime, resave),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(
        d.conflicts[0].kind,
        ConflictKind::MajorityVetoedByModifyDate
    );
}
