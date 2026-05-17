// spec §2.P1：容器内"数字化/写入"——次权威。

use tidymedia::media_time::{epoch_to_candidate, resolve, Priority, Source};

use super::common::{fixed_now, utc_offset};

/// spec §2.P1：EXIF CreateDate / DateTimeDigitized 在 P0 缺失时被采纳。
#[test]
fn exif_create_date_picked_when_no_p0() {
    let c = epoch_to_candidate(
        1_700_000_100,
        Source::ExifCreateDate,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P1);
    assert_eq!(d.source, Source::ExifCreateDate);
}

/// spec §2.P1：QuickTime atom CreateDate（容器写入时间）→ P1。
#[test]
fn quicktime_create_date_picked_when_no_p0() {
    let c = epoch_to_candidate(
        1_700_000_100,
        Source::QuickTimeCreateDate,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P1);
    assert_eq!(d.source, Source::QuickTimeCreateDate);
}
