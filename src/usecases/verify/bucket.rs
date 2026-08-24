//! 归档桶对账纯函数：把决策时间 → 预测桶 `YYYY:MM`；把 exiftool `QuickTime` 时间串
//! 按 UTC→配置时区转换取桶（移植 `.claude/scripts/tidy-verify/compare_buckets.py`）。

use chrono::{DateTime, Datelike, FixedOffset, NaiveDateTime, TimeZone, Utc};

/// 把 UTC 时刻按 `offset` 转本地后取 `YYYY:MM` 桶。
pub(crate) fn actual_bucket(utc: DateTime<Utc>, offset: FixedOffset) -> String {
    let local = utc.with_timezone(&offset);
    format!("{:04}:{:02}", local.year(), local.month())
}

/// 把 `QuickTime` 时间串按「UTC→配置时区」转换后取 `YYYY:MM`。
/// 带 `±HH:MM`/`Z` 后缀者先按其 offset 归一化到 UTC、无后缀者当 UTC，再 +tz 取年月。
/// 非法日期（如相机时钟未设的 `0000:00:00`）回退前 7 字符，暴露为 `0000:00` 桶
/// 而非崩溃——让可疑文件被 MISMATCH 检出（skill 的 `CameraClockUnset` 口径）。
pub(crate) fn qt_bucket(v: &str, tz_hours: i8) -> String {
    if let Ok(dt) = DateTime::<FixedOffset>::parse_from_str(v, "%Y:%m:%d %H:%M:%S%z") {
        return actual_bucket(dt.with_timezone(&Utc), offset(tz_hours));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(v, "%Y:%m:%d %H:%M:%S") {
        let utc = Utc.from_utc_datetime(&naive);
        return actual_bucket(utc, offset(tz_hours));
    }
    v.chars().take(7).collect()
}

fn offset(tz_hours: i8) -> FixedOffset {
    crate::usecases::config::chrono_offset_from_hours(tz_hours)
}

#[cfg(test)]
mod tests {
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
}
