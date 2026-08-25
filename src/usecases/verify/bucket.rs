//! 归档桶对账纯函数：把决策时间 → 预测桶 `YYYY:MM`；把 exiftool `QuickTime` 时间串
//! 按 UTC→配置时区转换取桶（口径与 tidy-verify skill 的桶对账一致）。

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
#[path = "bucket_tests.rs"]
mod tests;
