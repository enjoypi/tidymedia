use super::*;
use std::time::Duration;

#[test]
fn none_input_returns_none() {
    assert!(from_modified(None).is_none());
}

#[test]
fn before_unix_epoch_returns_none() {
    let before = UNIX_EPOCH.checked_sub(Duration::from_secs(1)).unwrap();
    assert!(from_modified(Some(before)).is_none());
}

#[test]
fn at_epoch_zero_ok() {
    let c = from_modified(Some(UNIX_EPOCH)).unwrap();
    assert_eq!(c.utc.timestamp(), 0);
    assert_eq!(c.source, Source::FsMtime);
    assert_eq!(c.offset, None);
    assert!(!c.inferred_offset);
}

#[test]
fn future_systemtime_is_kept() {
    let t = UNIX_EPOCH + Duration::from_hours(473_364);
    let c = from_modified(Some(t)).unwrap();
    assert_eq!(c.utc.timestamp(), 1_704_110_400);
}
