use super::*;
use chrono::TimeZone;

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0).unwrap()
}

#[test]
fn classify_epoch_1904_rejected() {
    assert_eq!(
        classify(quicktime_epoch(), fixed_now()),
        Validity::RejectEpoch1904
    );
}

#[test]
fn classify_future_beyond_one_day_rejected() {
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
