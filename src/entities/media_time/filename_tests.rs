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

// ─── IMG_ / DSC_ / VID_ ───────────────────────────────────────────────────

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

#[test]
fn img_east8_offset_applied() {
    // 本地 14:30 +08:00 = UTC 06:30
    let c = parse_filename("IMG_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenamePhone);
    assert_eq!(c.offset, Some(east8()));
    assert_eq!(c.utc.timestamp(), epoch("2024-05-01T06:30:00Z"));
}

#[rstest]
// wrong suffix length
#[case::img_short("IMG_2024050_143000.jpg")]
// bad month 13
#[case::img_bad_month("IMG_20241332_143000.jpg")]
// VID bad day 32
#[case::vid_bad_day("VID_20231532_103000.mp4")]
// wrong length
#[case::dsc_too_long("DSC_202405011_143000.jpg")]
fn camera_phone_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── PXL_ (Google Pixel) ──────────────────────────────────────────────────

#[rstest]
#[case::pxl_basic("PXL_20230615_103045123.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_mp("PXL_20230615_103045123.MP.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_portrait("PXL_20230615_103045123.PORTRAIT.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_no_ext("PXL_20230615_103045123", epoch("2023-06-15T10:30:45Z"))]
fn pixel_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenamePixel);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// too short after PXL_ prefix
#[case::pxl_short("PXL_2023061.jpg")]
// bad hour 99
#[case::pxl_bad_time("PXL_20230615_993045123.jpg")]
fn pixel_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── Screenshot_ ──────────────────────────────────────────────────────────

#[rstest]
#[case::screenshot_basic("Screenshot_2024-05-17-12-00-00.jpg", epoch("2024-05-17T12:00:00Z"))]
#[case::screenshot_no_ext("Screenshot_2024-05-17-12-00-00", epoch("2024-05-17T12:00:00Z"))]
fn screenshot_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameScreenshot);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// wrong length
#[case::screenshot_short("Screenshot_2024-05-17.jpg")]
// bad month 13
#[case::screenshot_bad_month("Screenshot_2024-13-32-25-99-99.jpg")]
fn screenshot_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── mmexport (WeChat) ────────────────────────────────────────────────────

#[rstest]
#[case::mmexport_basic("mmexport1686824625000.jpg", 1_686_824_625)]
#[case::mmexport_no_ext("mmexport1686824625000", 1_686_824_625)]
fn mmexport_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameWeChatExport);
    assert_eq!(c.offset, None);
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// 12 digits (too short)
#[case::mmexport_short("mmexport168682462500.jpg")]
// non-digit
#[case::mmexport_alpha("mmexport168682462500a.jpg")]
// empty after prefix
#[case::mmexport_empty("mmexport.jpg")]
fn mmexport_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── WhatsApp ─────────────────────────────────────────────────────────────

#[rstest]
#[case::wa_image(
    "WhatsApp Image 2023-06-15 at 10.30.45.jpeg",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_video(
    "WhatsApp Video 2023-06-15 at 10.30.45.mp4",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_image_seq(
    "WhatsApp Image 2023-06-15 at 10.30.45 (1).jpeg",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_video_seq(
    "WhatsApp Video 2023-06-15 at 10.30.45 (2).mp4",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
fn whatsapp_parsed(#[case] name: &str, #[case] expected_source: Source, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, expected_source);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// too short (missing time part)
#[case::wa_short("WhatsApp Image 2023-06-15.jpeg")]
// bad month 13
#[case::wa_bad_month("WhatsApp Image 2023-13-15 at 10.30.45.jpeg")]
// unrecognized prefix
#[case::wa_wrong_prefix("WhatsApp Audio 2023-06-15 at 10.30.45.m4a")]
fn whatsapp_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── Bare YYYYMMDD_HHMMSS ─────────────────────────────────────────────────

#[rstest]
#[case::bare_basic("20230615_103000.jpg", epoch("2023-06-15T10:30:00Z"))]
#[case::bare_no_ext("20230615_103000", epoch("2023-06-15T10:30:00Z"))]
fn bare_yyyymmdd_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameBareYyyymmdd);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// bad month 13
#[case::bare_bad_month("20231332_103000.jpg")]
// wrong length (14 chars)
#[case::bare_short("2023061_103000.jpg")]
// looks like unix millis (13 digits) — should match unix_millis not bare
#[case::bare_vs_millis("1686824625000.jpg")]
fn bare_yyyymmdd_rejects_or_routes_elsewhere(#[case] name: &str) {
    // For the millis case, it should still parse (via unix_millis), just not as bare
    if name == "1686824625000.jpg" {
        let c = parse_filename(name, utc_offset()).unwrap();
        assert_eq!(c.source, Source::FilenameUnixMillis);
    } else {
        assert!(parse_filename(name, utc_offset()).is_none());
    }
}

// ─── Unix millis (pure 13-digit) ─────────────────────────────────────────

#[rstest]
#[case::millis_basic("1715961600000.jpg", 1_715_961_600)]
#[case::millis_no_ext("1715961600000", 1_715_961_600)]
fn unix_millis_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameUnixMillis);
    assert_eq!(c.offset, None);
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// 12 digits
#[case::millis_12("171596160000.jpg")]
// 14 digits
#[case::millis_14("17159616000000.jpg")]
// alpha char
#[case::millis_alpha("171596160000a.jpg")]
fn unix_millis_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// ─── No match ─────────────────────────────────────────────────────────────

#[test]
fn no_known_pattern_returns_none() {
    assert!(parse_filename("random.jpg", utc_offset()).is_none());
}

#[test]
fn empty_name_returns_none() {
    assert!(parse_filename("", utc_offset()).is_none());
}

// ─── stem_without_ext ─────────────────────────────────────────────────────

#[test]
fn stem_strips_last_extension_only() {
    // PXL with .MP.jpg — stem_without_ext strips only .jpg, then try_pixel strips .MP via prefix-then-15chars
    // Validate stem is "PXL_20230615_103045123.MP"
    let stem = super::stem_without_ext("PXL_20230615_103045123.MP.jpg");
    assert_eq!(stem, "PXL_20230615_103045123.MP");
}

// ─── Regression: east8 offset applied to new formats ────────────────────

#[test]
fn vid_east8_offset_applied() {
    let c = parse_filename("VID_20230615_103000.mp4", east8()).unwrap();
    // 本地 10:30 +08:00 = UTC 02:30
    let expected = Utc.with_ymd_and_hms(2023, 6, 15, 2, 30, 0).unwrap();
    assert_eq!(c.utc, expected);
}

#[test]
fn whatsapp_east8_offset_applied() {
    let c = parse_filename("WhatsApp Image 2023-06-15 at 10.30.45.jpeg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2023, 6, 15, 2, 30, 45).unwrap();
    assert_eq!(c.utc, expected);
}
