use chrono::TimeZone;
use chrono::Utc;
use rstest::rstest;

use super::*;

fn east8() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

/// UTC epoch at a given date-time string (RFC3339 without sub-seconds).
fn epoch(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}

#[rstest]
#[case::img_basic(
    "IMG_20240501_143000.jpg",
    Source::FilenamePhone,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::img_no_ext(
    "IMG_20240501_143000",
    Source::FilenamePhone,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::dsc_basic(
    "DSC_20240501_143000.jpg",
    Source::FilenameCamera,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::dsc_no_ext(
    "DSC_20240501_143000",
    Source::FilenameCamera,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::vid_basic(
    "VID_20230615_103000.mp4",
    Source::FilenameVideoPhone,
    epoch("2023-06-15T10:30:00Z")
)]
#[case::vid_no_ext(
    "VID_20230615_103000",
    Source::FilenameVideoPhone,
    epoch("2023-06-15T10:30:00Z")
)]
fn camera_phone_video_parsed_utc(
    #[case] name: &str,
    #[case] expected_source: Source,
    #[case] expected_ts: i64,
) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, expected_source);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
    assert_eq!(c.offset, Some(utc_offset()));
}

include!("filename_part1_tests.rs");
include!("filename_part2_tests.rs");
include!("filename_part3_tests.rs");
