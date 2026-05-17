// spec §四：时区处理。

use tidymedia::media_time::filename::parse_filename;
use tidymedia::media_time::{epoch_to_candidate, resolve, Source};

use super::common::{east8, fixed_now, utc_offset};

/// spec §4："EXIF 无 OffsetTime 时按调用方默认时区解释，标 inferred"。
#[test]
fn filename_no_native_tz_uses_default_marked_inferred() {
    let c = parse_filename("DSC_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.offset, Some(east8()));
    assert!(c.inferred_offset);
}

/// spec §4："MKV DateUTC 是 UTC，不需要推断"。
#[test]
fn mkv_dateutc_carries_no_tz_no_inference() {
    let c = epoch_to_candidate(1_700_000_000, Source::MkvDateUtc, None, false).unwrap();
    assert!(c.offset.is_none());
    assert!(!c.inferred_offset);
}

/// spec §4：13 位 Unix 毫秒无时区，inferred 不亮。
#[test]
fn unix_millis_no_tz_not_inferred() {
    let c = parse_filename("1715961600000.jpg", east8()).unwrap();
    assert!(c.offset.is_none());
    assert!(!c.inferred_offset);
}

/// spec §4：调用方可显式指定原生时区（如 P0 EXIF OffsetTimeOriginal）。
#[test]
fn candidate_with_native_offset_not_marked_inferred() {
    let c = epoch_to_candidate(
        1_700_000_000,
        Source::ExifDateTimeOriginal,
        Some(east8()),
        false, // 来源含原生 offset → not inferred
    )
    .unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.offset, Some(east8()));
    assert!(!d.inferred_offset);
}

/// spec §4：decision 透传 best 候选的 offset 与 inferred 标记。
#[test]
fn decision_propagates_offset_from_best() {
    let c = epoch_to_candidate(1_700_000_000, Source::MkvDateUtc, None, false).unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.offset, None);

    let c2 = epoch_to_candidate(
        1_700_000_000,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d2 = resolve(vec![c2], None, fixed_now()).unwrap();
    assert_eq!(d2.offset, Some(utc_offset()));
}
