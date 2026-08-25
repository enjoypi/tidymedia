use chrono::TimeZone;
use chrono::Utc;

use crate::entities::media_time::decision::ConflictKind;
use crate::entities::media_time::priority::Priority;
use crate::entities::media_time::priority::Source;
use crate::entities::media_time::resolve::resolve;

use super::tests_common::cand;
use super::tests_common::now;

const CONFLICT_OVER_DAY_SECS: i64 = 86_400;

#[test]
fn majority_filename_mtime_overrules_wrong_p0() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.utc.timestamp(), real);
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.source, Source::FilenameDashedDateTime);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
    assert_eq!(d.conflicts[0].other_utc.timestamp(), p0);
    assert_eq!(
        d.conflicts[0].other_source,
        Some(Source::ExifDateTimeOriginal)
    );
}

#[test]
fn p0_kept_when_filename_lacks_mtime_corroboration() {
    let p0 = 1_000_000_000;
    let f = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, f),
            cand(Source::FsMtime, f + 3 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_no_mtime_candidate() {
    let p0 = 1_000_000_000;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, p0 + 600 * 86_400),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_filename_within_30_days() {
    let p0 = 1_700_000_100;
    let f = p0 + 10 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, f),
            cand(Source::FsMtime, f),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn p0_kept_when_modify_date_corroborates_majority() {
    let p0 = 1_000_000_000;
    let resave = p0 + 600 * 86_400;
    let md = Utc.timestamp_opt(resave + 1800, 0).single().unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, resave),
            cand(Source::FsMtime, resave + 3600),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts.len(), 2);
    assert_eq!(
        d.conflicts[0].kind,
        ConflictKind::MajorityVetoedByModifyDate
    );
    assert_eq!(d.conflicts[0].other_utc.timestamp(), resave);
    assert_eq!(
        d.conflicts[0].other_source,
        Some(Source::FilenameDashedDateTime)
    );
    assert_eq!(d.conflicts[0].diff_secs, p0 - resave);
    assert_eq!(d.conflicts[1].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn majority_overrules_when_modify_date_missing() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
}

#[test]
fn majority_overrules_when_modify_date_far_from_filename() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let md = Utc
        .timestamp_opt(real + CONFLICT_OVER_DAY_SECS + 1, 0)
        .single()
        .unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.utc.timestamp(), real);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
}

#[test]
fn modify_date_at_exactly_one_day_still_vetoes() {
    let p0 = 1_000_000_000;
    let resave = p0 + 600 * 86_400;
    let md = Utc
        .timestamp_opt(resave + CONFLICT_OVER_DAY_SECS, 0)
        .single()
        .unwrap();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, resave),
            cand(Source::FsMtime, resave),
        ],
        None,
        Some(md),
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(
        d.conflicts[0].kind,
        ConflictKind::MajorityVetoedByModifyDate
    );
}

// 下载时戳类文件名（13 位 unix 毫秒 / mmexport）与 mtime 天然同源——下载器落盘
// 即把 mtime 写成下载时刻，"互证"恒真是假象，不构成推翻 P0 的证据
//（real-2019 fixture `1547957801421.mp4`：QT 六字段一致 2016:05 被微信下载
// 时戳+mtime 假多数派推翻）。
#[test]
fn unix_millis_vote_does_not_overrule_p0() {
    let p0 = Utc
        .with_ymd_and_hms(2016, 5, 23, 5, 58, 15)
        .unwrap()
        .timestamp();
    let download = Utc
        .with_ymd_and_hms(2019, 1, 20, 4, 16, 41)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::QuickTimeCreationDate, p0),
            cand(Source::FilenameUnixMillis, download),
            cand(Source::FsMtime, download + 1),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.source, Source::QuickTimeCreationDate);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn wechat_export_vote_does_not_overrule_p0() {
    let p0 = Utc
        .with_ymd_and_hms(2016, 5, 23, 5, 58, 15)
        .unwrap()
        .timestamp();
    let download = Utc
        .with_ymd_and_hms(2019, 1, 20, 4, 16, 41)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::QuickTimeCreationDate, p0),
            cand(Source::FilenameWeChatExport, download),
            cand(Source::FsMtime, download + 1),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

// 括号内紧凑时戳与 QQ 导出：黑名单归属待真实样本实证，当前默认无票（数据安全
// 优先，防下载时戳错误推翻相机 P0）——实证为原图拍摄时间后再移除黑名单。
#[test]
fn bracketed_compact_vote_does_not_overrule_p0() {
    let p0 = Utc
        .with_ymd_and_hms(2016, 5, 23, 5, 58, 15)
        .unwrap()
        .timestamp();
    let download = Utc
        .with_ymd_and_hms(2019, 1, 20, 4, 16, 41)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::QuickTimeCreationDate, p0),
            cand(Source::FilenameBracketedCompact, download),
            cand(Source::FsMtime, download + 1),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

#[test]
fn qq_export_vote_does_not_overrule_p0() {
    let p0 = Utc
        .with_ymd_and_hms(2016, 5, 23, 5, 58, 15)
        .unwrap()
        .timestamp();
    let download = Utc
        .with_ymd_and_hms(2019, 1, 20, 4, 16, 41)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::QuickTimeCreationDate, p0),
            cand(Source::FilenameQqExport, download),
            cand(Source::FsMtime, download + 1),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
    assert_eq!(d.conflicts.len(), 1);
    assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
}

// 合法场景回归：拍摄命名类文件名（IMG_YYYYMMDD_HHMMSS）+ mtime 互证仍推翻
// 相机时钟错误的 P0。
#[test]
fn camera_named_vote_still_overrules_p0() {
    let p0 = 1_000_000_000;
    let real = p0 + 600 * 86_400;
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenamePhone, real),
            cand(Source::FsMtime, real + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.utc.timestamp(), real);
    assert_eq!(d.source, Source::FilenamePhone);
    assert_eq!(d.conflicts[0].kind, ConflictKind::P0OverruledByMajority);
}

// 触发 majority_verdict line 130 `matches!(v, Validity::Valid)` 的 false arm：
// filename 候选 utc 在 pre-1995（LowConfidencePre1995 validity）时，多数派仲裁
// 必须拒绝它推翻 P0（CLAUDE.md「多数派仲裁仅认 Validity::Valid」）。
#[test]
fn pre_1995_filename_low_confidence_does_not_overrule_p0() {
    let p0 = Utc
        .with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
        .unwrap()
        .timestamp();
    let pre_1995 = Utc
        .with_ymd_and_hms(1990, 6, 15, 0, 0, 0)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, pre_1995),
            cand(Source::FsMtime, pre_1995 + 3600),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
}

// 触发 inner closure line 134 `matches!(mv, Validity::Valid)` 的 false arm：
// 只有一个 mtime 候选且其 utc 是 pre-1995（LowConfidencePre1995 validity）时，
// inner any() 找不到 valid mtime 互证 → quorum=None → P0 保留。
#[test]
fn p0_kept_when_only_mtime_candidate_is_pre_1995_low_confidence() {
    let p0 = Utc
        .with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
        .unwrap()
        .timestamp();
    let f = p0 + 60 * CONFLICT_OVER_DAY_SECS;
    let pre_1995_mtime = Utc
        .with_ymd_and_hms(1990, 6, 15, 0, 0, 0)
        .unwrap()
        .timestamp();
    let d = resolve(
        vec![
            cand(Source::ExifDateTimeOriginal, p0),
            cand(Source::FilenameDashedDateTime, f),
            cand(Source::FsMtime, pre_1995_mtime),
        ],
        None,
        None,
        now(),
    )
    .unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), p0);
}
