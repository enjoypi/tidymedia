// docs/media-time-detection.md §五.1/§五.2/§五.3：三层闸口
//   §5.1 1904 占位（MP4 未写 creation_time 时 nom-exif 返回 1904-01-01Z）→ 剔除
//   §5.2 未来时间 > now + 1 天 → 剔除（相机日期未设置）
//   §5.3 1995 之前 → 软阈值，保留但降低置信度

use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;

/// 1904-01-01T00:00:00Z 的 epoch（QuickTime 零点占位）。
const EPOCH_1904: i64 = -2_082_844_800;

/// 1995-01-01T00:00:00Z 的 epoch（数码摄影合理下限）。
const SOFT_THRESHOLD_1995: i64 = 788_918_400;

/// 未来时间宽容值：now + 1 天（覆盖跨时区上传与 NTP 漂移）。
const FUTURE_TOLERANCE_SECS: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    Valid,
    /// 保留候选但应降低置信度（spec §5.3）。
    LowConfidencePre1995,
    /// 剔除：QuickTime 1904 占位（spec §5.1）。
    RejectEpoch1904,
    /// 剔除：未来时间（spec §5.2）。
    RejectFuture,
}

pub fn classify(utc: DateTime<Utc>, now: DateTime<Utc>) -> Validity {
    let ts = utc.timestamp();
    if ts == EPOCH_1904 {
        return Validity::RejectEpoch1904;
    }
    if ts > now.timestamp().saturating_add(FUTURE_TOLERANCE_SECS) {
        return Validity::RejectFuture;
    }
    if ts < SOFT_THRESHOLD_1995 {
        return Validity::LowConfidencePre1995;
    }
    Validity::Valid
}

/// QuickTime epoch 占位时间，用于内部识别与测试断言。
pub fn quicktime_epoch() -> DateTime<Utc> {
    Utc.timestamp_opt(EPOCH_1904, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0).unwrap()
    }

    #[test]
    fn classify_epoch_1904_rejected() {
        // spec §5.1
        assert_eq!(
            classify(quicktime_epoch(), fixed_now()),
            Validity::RejectEpoch1904
        );
    }

    #[test]
    fn classify_future_beyond_one_day_rejected() {
        // spec §5.2
        let future = fixed_now() + chrono::TimeDelta::seconds(FUTURE_TOLERANCE_SECS + 1);
        assert_eq!(classify(future, fixed_now()), Validity::RejectFuture);
    }

    #[test]
    fn classify_future_within_one_day_valid() {
        // 上限边界：恰好 now + FUTURE_TOLERANCE_SECS → 合法
        let future = fixed_now() + chrono::TimeDelta::seconds(FUTURE_TOLERANCE_SECS);
        assert_eq!(classify(future, fixed_now()), Validity::Valid);
    }

    #[test]
    fn classify_pre_1995_low_confidence() {
        // spec §5.3
        let pre = Utc.with_ymd_and_hms(1980, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(classify(pre, fixed_now()), Validity::LowConfidencePre1995);
    }

    #[test]
    fn classify_just_after_threshold_valid() {
        // 边界：恰好 1995-01-01T00:00:00Z 也算 valid（< 才降置信）
        let at = Utc.timestamp_opt(SOFT_THRESHOLD_1995, 0).single().unwrap();
        assert_eq!(classify(at, fixed_now()), Validity::Valid);
    }

    #[test]
    fn quicktime_epoch_is_1904() {
        assert_eq!(quicktime_epoch().timestamp(), EPOCH_1904);
    }
}
