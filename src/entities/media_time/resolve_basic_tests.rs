use chrono::FixedOffset;
use chrono::TimeZone;
use chrono::Utc;

use crate::entities::media_time::candidate::Candidate;
use crate::entities::media_time::decision::Confidence;
use crate::entities::media_time::decision::ConflictKind;
use crate::entities::media_time::filter;
use crate::entities::media_time::priority::Priority;
use crate::entities::media_time::priority::Source;
use crate::entities::media_time::resolve::resolve;

use super::tests_common::cand;
use super::tests_common::now;

#[test]
fn empty_returns_none() {
    assert!(resolve(vec![], None, None, now()).is_none());
}

#[test]
fn p0_wins_over_p1() {
    let d = resolve(
        vec![
            cand(Source::ExifCreateDate, 1_700_000_100),
            cand(Source::ExifDateTimeOriginal, 1_700_000_200),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), 1_700_000_200);
}

#[test]
fn same_priority_takes_earlier() {
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, 1_700_000_200),
            cand(Source::QuickTimeCreationDate, 1_700_000_100),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.utc.timestamp(), 1_700_000_100);
}

#[test]
fn epoch_1904_filtered_out() {
    let epoch = filter::quicktime_epoch().timestamp();
    let d = resolve(
        vec![
            cand(Source::QuickTimeCreateDate, epoch),
            cand(Source::ExifCreateDate, 1_700_000_100),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.utc.timestamp(), 1_700_000_100);
}

#[test]
fn future_filtered_out() {
    let future = now().timestamp() + 100 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, future),
            cand(Source::ExifCreateDate, 1_700_000_100),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P1);
}

#[test]
fn pre_1995_kept_with_low_confidence() {
    let pre = 315_532_800;
    let d = resolve(
        vec![cand(Source::ExifDateTimeOriginal, pre)],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.confidence, Confidence::Low);
}

#[test]
fn confidence_high_when_no_pre_1995() {
    let d = resolve(
        vec![cand(Source::ExifDateTimeOriginal, 1_700_000_100)],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.confidence, Confidence::High);
}

#[test]
fn gps_diff_over_24h_recorded_as_conflict() {
    let p0 = 1_700_000_100;
    let gps = Utc.timestamp_opt(p0 + 48 * 3600, 0).single().unwrap();
    let d = resolve(
        vec![cand(Source::ExifDateTimeOriginal, p0)],
        Some(gps),
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
}

#[test]
fn gps_diff_within_24h_no_conflict() {
    let p0 = 1_700_000_100;
    let gps = Utc.timestamp_opt(p0 + 3600, 0).single().unwrap();
    let d = resolve(
        vec![cand(Source::ExifDateTimeOriginal, p0)],
        Some(gps),
        None,
        now(),
    )
    .unwrap();
    assert!(d.conflicts.is_empty());
}

#[test]
fn filename_diff_over_one_day_recorded() {
    let p0 = 1_700_000_100;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenamePhone, p0 + 2 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn mtime_much_earlier_than_p0_recorded() {
    let p0 = 1_700_000_100;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FsMtime, p0 - 60 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::MtimeMuchEarlierThanP0);
}

#[test]
fn mtime_later_than_p0_not_recorded() {
    let p0 = 1_700_000_100;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FsMtime, p0 + 60 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert!(d.conflicts.is_empty());
}

#[test]
fn non_p0_best_skips_conflict_detection() {
    let d = resolve(
        vec![
            cand(Source::FilenamePhone, 1_700_000_100),
            cand(Source::FsMtime, 1_700_000_100 - 60 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert!(d.conflicts.is_empty());
}

#[test]
fn surviving_includes_low_confidence_path() {
    let d = resolve(
        vec![cand(Source::ExifDateTimeOriginal, 315_532_800)],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.confidence, Confidence::Low);
}

#[test]
fn offset_and_inferred_flag_propagate() {
    let east8 = FixedOffset::east_opt(8 * 3600).unwrap();
    let c = Candidate {
        utc: Utc.timestamp_opt(1_700_000_100, 0).single().unwrap(),
        offset: Some(east8),
        source: Source::FilenamePhone,
        inferred_offset: true,
    };
    let d = resolve(vec![c], None, None, now()).unwrap();
    assert_eq!(d.offset, Some(east8));
    assert!(d.inferred_offset);
}
