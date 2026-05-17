// spec §六：一致性校验。

use tidymedia::media_time::{epoch_to_candidate, resolve, ConflictKind, Source};

use super::common::{fixed_now, ts, utc_offset};

/// spec §6："P0 vs GPS UTC 差值 > 24h，告警（相机日期错乱）"。
#[test]
fn gps_diff_over_24h_recorded_as_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 48 * 3600); // +48h
    let d = resolve(vec![p0], Some(gps), fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
}

/// spec §6：GPS 差 < 24h → 不告警。
#[test]
fn gps_diff_within_24h_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 3600); // +1h
    let d = resolve(vec![p0], Some(gps), fixed_now()).unwrap();
    assert!(d.conflicts.is_empty());
}

/// spec §6："P0 vs 文件名解析 差值 > 1 天 → 告警"。
#[test]
fn filename_diff_over_one_day_recorded_as_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let fname = epoch_to_candidate(
        1_700_000_100 + 2 * 86_400,
        Source::FilenamePhone,
        Some(utc_offset()),
        true,
    )
    .unwrap();
    let d = resolve(vec![p0, fname], None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

/// spec §6："P0 vs mtime 差值 > N 天但 mtime < P0 → 仅提示"。
#[test]
fn mtime_much_earlier_than_p0_only_hints() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime = epoch_to_candidate(1_700_000_100 - 60 * 86_400, Source::FsMtime, None, false)
        .unwrap();
    let d = resolve(vec![p0, mtime], None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::MtimeMuchEarlierThanP0);
}

/// spec §6：mtime 晚于 P0 不算"假象"，不告警。
#[test]
fn mtime_later_than_p0_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime = epoch_to_candidate(1_700_000_100 + 60 * 86_400, Source::FsMtime, None, false)
        .unwrap();
    let d = resolve(vec![p0, mtime], None, fixed_now()).unwrap();
    assert!(d.conflicts.is_empty());
}

/// spec §6：best 不是 P0 时跳过交叉校验（spec 的措辞 "P0 vs X" 隐含此前提）。
#[test]
fn non_p0_best_skips_cross_validation() {
    let p2 = epoch_to_candidate(
        1_700_000_100,
        Source::FilenamePhone,
        Some(utc_offset()),
        true,
    )
    .unwrap();
    let mtime = epoch_to_candidate(1_700_000_100 - 60 * 86_400, Source::FsMtime, None, false)
        .unwrap();
    let d = resolve(vec![p2, mtime], None, fixed_now()).unwrap();
    assert!(d.conflicts.is_empty());
}
