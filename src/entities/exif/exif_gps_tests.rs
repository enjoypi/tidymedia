use camino::Utf8Path;

use super::super::test_common as common;
use super::Exif;

/// JPEG 含 GPS 时间字段 → `gps_utc()` 解析到 2023-06-15T10:30:00Z。
#[test]
fn from_path_reads_jpeg_with_gps_utc() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_GPS)).unwrap();
    let gps = exif.gps_utc().expect("GPS fixture must have gps_utc");
    // 2023-06-15T10:30:00Z = 1686825000
    assert_eq!(
        gps.timestamp(),
        1_686_825_000,
        "GPS UTC should be 2023-06-15T10:30:00Z"
    );
}

/// JPEG 无 GPS 字段 → `gps_utc()` 返回 None。
#[test]
fn from_path_jpeg_without_gps_returns_none() {
    let exif = Exif::from_path(Utf8Path::new(common::DATA_JPEG_WITH_EXIF)).unwrap();
    assert!(
        exif.gps_utc().is_none(),
        "fixture without GPS must return None"
    );
}

/// `parse_gps_date`：date 格式非法 → None。
#[test]
fn parse_gps_date_invalid_returns_none() {
    assert!(super::parse_gps_date("not-a-date").is_none());
    assert!(super::parse_gps_date("2023:13:01").is_none()); // month 13 invalid
    // 数字合法但段数不足：必须由 len 前置 guard 拦下（而非依赖 parse 失败或越界 panic）
    assert!(super::parse_gps_date("2024:05").is_none());
}

/// `parse_gps_date`：三段齐全但各段 parse 失败 → 各自 `ok()?` 命中 None 分支。
#[test]
fn parse_gps_date_non_numeric_year_returns_none() {
    assert!(super::parse_gps_date("ABCD:05:01").is_none());
}

#[test]
fn parse_gps_date_non_numeric_month_returns_none() {
    assert!(super::parse_gps_date("2024:XX:01").is_none());
}

#[test]
fn parse_gps_date_non_numeric_day_returns_none() {
    assert!(super::parse_gps_date("2024:05:DD").is_none());
}

/// `rational_to_u32`：denominator=0 → None。
#[test]
fn rational_to_u32_zero_denominator_returns_none() {
    let r = nom_exif::URational::new(10, 0);
    assert!(super::rational_to_u32(r).is_none());
}

/// `rational_to_u32`：denominator>1 时做真除法（10/2=5，杀「/ 变 *」类算术变异；
/// 既有用例 denom 全是 0 或 1，1 时乘除等价）。
#[test]
fn rational_to_u32_divides_by_denominator() {
    let r = nom_exif::URational::new(10, 2);
    assert_eq!(super::rational_to_u32(r), Some(5));
}

/// `build_gps_utc`：date 或 time 任一为 None → None。
#[test]
fn build_gps_utc_missing_date_returns_none() {
    assert!(super::build_gps_utc(None, None).is_none());
    let r = nom_exif::URational::new(10, 1);
    assert!(super::build_gps_utc(None, Some([r, r, r])).is_none());
    assert!(super::build_gps_utc(Some("2023:06:15"), None).is_none());
}

/// `build_gps_utc`：`parse_gps_date` 返 None（非法日期格式）→ `?` Err 分支。
#[test]
fn build_gps_utc_invalid_date_string_returns_none() {
    // "not-a-date" → parse_gps_date returns None → build_gps_utc returns None
    let r = nom_exif::URational::new(10, 1);
    assert!(super::build_gps_utc(Some("not-a-date"), Some([r, r, r])).is_none());
}

/// `build_gps_utc`：`rational_to_u32(h)` 返 None（分母为 0）→ `?` Err 分支。
#[test]
fn build_gps_utc_zero_denominator_rational_returns_none() {
    // zero-denominator → rational_to_u32 returns None → build_gps_utc returns None
    let zero = nom_exif::URational::new(10, 0);
    let ok = nom_exif::URational::new(10, 1);
    // h 分母为 0
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([zero, ok, ok])).is_none());
    // m 分母为 0（h 先成功，m 失败）
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([ok, zero, ok])).is_none());
    // s 分母为 0（h、m 先成功，s 失败）
    assert!(super::build_gps_utc(Some("2023:06:15"), Some([ok, ok, zero])).is_none());
}
