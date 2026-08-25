use chrono::{TimeZone, Utc};

use super::actual_bucket;
use super::qt_bucket;
use crate::usecases::config::chrono_offset_from_hours;

#[test]
fn actual_bucket_converts_utc_to_configured_tz() {
    let utc = Utc.with_ymd_and_hms(2020, 7, 31, 23, 0, 0).unwrap();
    assert_eq!(actual_bucket(utc, chrono_offset_from_hours(8)), "2020:08");
}

#[test]
fn actual_bucket_keeps_year_month_without_dst_drift() {
    let utc = Utc.with_ymd_and_hms(2021, 1, 1, 4, 0, 0).unwrap();
    assert_eq!(actual_bucket(utc, chrono_offset_from_hours(-5)), "2020:12");
}

#[test]
fn qt_bucket_with_offset_suffix_normalizes_to_utc() {
    assert_eq!(qt_bucket("2020:07:25 20:40:10+08:00", 8), "2020:07");
}

#[test]
fn qt_bucket_without_suffix_treated_as_utc() {
    assert_eq!(qt_bucket("2020:07:31 23:00:00", 8), "2020:08");
}

#[test]
fn qt_bucket_invalid_date_falls_back_to_prefix() {
    assert_eq!(qt_bucket("0000:00:00 00:00:00", 8), "0000:00");
}

#[test]
fn qt_bucket_non_time_string_falls_back_to_prefix() {
    assert_eq!(qt_bucket("-", 8), "-");
}
