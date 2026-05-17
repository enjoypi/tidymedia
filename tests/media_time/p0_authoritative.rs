// spec §2.P0：容器内"拍摄时刻"——最权威来源。

use tidymedia::media_time::{epoch_to_candidate, resolve, Priority, Source};

use super::common::{fixed_now, utc_offset};

/// spec §2.P0 图片：EXIF DateTimeOriginal 是拍摄时刻。
#[test]
fn exif_datetimeoriginal_yields_p0() {
    let c = epoch_to_candidate(
        1_714_545_000, // 2024-05-01T06:30:00Z
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.source, Source::ExifDateTimeOriginal);
    assert_eq!(d.utc.timestamp(), 1_714_545_000);
}

/// spec §2.P0 视频：QuickTime com.apple.quicktime.creationdate（带时区）→ P0。
#[test]
fn quicktime_creationdate_yields_p0() {
    let c = epoch_to_candidate(
        1_714_545_000,
        Source::QuickTimeCreationDate,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.source, Source::QuickTimeCreationDate);
}

/// spec §2.P0 视频：MKV/WebM DateUTC（UTC、纳秒精度）→ P0。
#[test]
fn mkv_dateutc_yields_p0() {
    let c = epoch_to_candidate(1_714_545_000, Source::MkvDateUtc, None, false).unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.source, Source::MkvDateUtc);
}

/// spec §2.P0 vs §2.P1：同时存在 P0 与 P1 时，P0 胜出（即便 P0 时间更晚）。
#[test]
fn p0_beats_p1_even_if_later() {
    let earlier_p1 = epoch_to_candidate(
        1_700_000_100, // P1 更早
        Source::ExifCreateDate,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let later_p0 = epoch_to_candidate(
        1_700_000_200,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![earlier_p1, later_p0], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), 1_700_000_200);
}
