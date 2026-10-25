use tidymedia::media_time::{ConflictKind, Source, epoch_to_candidate, resolve};

use super::common::{fixed_now, ts, utc_offset};

/// P0 vs GPS UTC 差值 > 24h，告警（相机日期错乱）。
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
    let d = resolve(vec![p0], Some(gps), None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
}

/// GPS 差 < 24h → 不告警。
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
    let d = resolve(vec![p0], Some(gps), None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// GPS 恰好相差 24h（阈值边界，含等不告警）：杀 `> 24*3600` 被变异成 `>=`。
#[test]
fn gps_diff_exactly_24h_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let gps = ts(1_700_000_100 + 24 * 3600); // 恰 +24h
    let d = resolve(vec![p0], Some(gps), None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// P0 vs 文件名解析 差值 > 1 天 → 告警。
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
    let d = resolve(vec![p0, fname], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

/// 文件名候选恰好相差 1 天（阈值边界，含等不告警）：杀 `> 86_400` 被变异成 `>=`。
#[test]
fn filename_diff_exactly_one_day_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let fname = epoch_to_candidate(
        1_700_000_100 + 86_400, // 恰 +1 天
        Source::FilenamePhone,
        Some(utc_offset()),
        true,
    )
    .unwrap();
    let d = resolve(vec![p0, fname], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// P0 vs mtime 差值 > N 天但 mtime < P0 → 仅提示。
#[test]
fn mtime_much_earlier_than_p0_only_hints() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime =
        epoch_to_candidate(1_700_000_100 - 60 * 86_400, Source::FsMtime, None, false).unwrap();
    let d = resolve(vec![p0, mtime], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::MtimeMuchEarlierThanP0);
}

/// 差距在阈值内（5 天 < 30 天）不告警：杀 `30 * 86_400` 阈值常量的算术变异
/// （`*`→`+` 变 ~1 天、`*`→`-` 变负数，都会让 5 天差距误报冲突）。
#[test]
fn mtime_slightly_earlier_than_p0_within_threshold_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime =
        epoch_to_candidate(1_700_000_100 - 5 * 86_400, Source::FsMtime, None, false).unwrap();
    let d = resolve(vec![p0, mtime], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// mtime 恰好早 30 天（阈值边界，含等不告警）：杀 `> MTIME_VS_P0_HINT_SECS`
/// 被变异成 `>=`。
#[test]
fn mtime_exactly_threshold_earlier_than_p0_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime = epoch_to_candidate(
        1_700_000_100 - 30 * 86_400, // 恰 30 天
        Source::FsMtime,
        None,
        false,
    )
    .unwrap();
    let d = resolve(vec![p0, mtime], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// mtime 晚于 P0 不算"假象"，不告警。
#[test]
fn mtime_later_than_p0_no_conflict() {
    let p0 = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let mtime =
        epoch_to_candidate(1_700_000_100 + 60 * 86_400, Source::FsMtime, None, false).unwrap();
    let d = resolve(vec![p0, mtime], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}

/// best 不是 P0 时跳过交叉校验（"P0 vs X" 的措辞隐含此前提）。
#[test]
fn non_p0_best_skips_cross_validation() {
    let p2 = epoch_to_candidate(
        1_700_000_100,
        Source::FilenamePhone,
        Some(utc_offset()),
        true,
    )
    .unwrap();
    let mtime =
        epoch_to_candidate(1_700_000_100 - 60 * 86_400, Source::FsMtime, None, false).unwrap();
    let d = resolve(vec![p2, mtime], None, None, fixed_now()).unwrap();
    assert_eq!(d.conflicts, []);
}
