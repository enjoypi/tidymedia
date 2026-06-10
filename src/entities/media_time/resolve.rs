// docs/media-time-detection.md §三 + §六：合并 + 冲突校验。
//   - 按 Priority 升序排序（P0 优先），同优先级取较早 utc
//   - 1904 / 未来时间过滤，1995 之前降置信
//   - 当 best.priority == P0 时做交叉校验：GPS、filename、mtime

use chrono::DateTime;
use chrono::Utc;

use super::candidate::Candidate;
use super::decision::Confidence;
use super::decision::Conflict;
use super::decision::ConflictKind;
use super::decision::MediaTimeDecision;
use super::filter;
use super::filter::Validity;
use super::priority::Priority;
use super::priority::Source;

/// "mtime < P0 但差距较大" 中的"较大"采用 30 天，超过即作为提示性冲突。
const MTIME_VS_P0_HINT_SECS: i64 = 30 * 86_400;

/// P0 与 GPS / 文件名候选的交叉校验阈值：相差超过 1 天即记冲突
/// （对应 `ConflictKind::GpsOver24h` / `FilenameOver1Day`）。
const CONFLICT_OVER_DAY_SECS: i64 = 86_400;

#[must_use]
pub fn resolve(
    candidates: Vec<Candidate>,
    gps_utc: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<MediaTimeDecision> {
    let mut surviving = apply_filters(candidates, now);
    if surviving.is_empty() {
        return None;
    }

    // 排序：(priority asc, utc asc) → 第一个是 best
    surviving.sort_by(|a, b| {
        a.0.priority()
            .cmp(&b.0.priority())
            .then(a.0.utc.cmp(&b.0.utc))
    });
    let (mut best, mut validity) = surviving[0];
    // 多数派仲裁：P0 错误（相机时钟未调）的典型痕迹是 filename 与 mtime
    // 互证一致却与 P0 相差悬殊；两票对一票，推翻 P0 并记冲突（不静默）。
    let mut conflicts = Vec::new();
    if let Some((winner, v)) = majority_override(&best, &surviving[1..]) {
        conflicts.push(Conflict {
            kind: ConflictKind::P0OverruledByMajority,
            other_utc: best.utc,
            other_source: Some(best.source),
            diff_secs: (best.utc - winner.utc).num_seconds(),
        });
        (best, validity) = (winner, v);
    } else if best.priority() == Priority::P0 {
        conflicts = detect_conflicts(&best, &surviving[1..], gps_utc);
    }
    let confidence = match validity {
        Validity::LowConfidencePre1995 => Confidence::Low,
        _ => Confidence::High,
    };

    Some(MediaTimeDecision {
        utc: best.utc,
        offset: best.offset,
        priority: best.priority(),
        source: best.source,
        inferred_offset: best.inferred_offset,
        confidence,
        conflicts,
    })
}

fn apply_filters(candidates: Vec<Candidate>, now: DateTime<Utc>) -> Vec<(Candidate, Validity)> {
    candidates
        .into_iter()
        .filter_map(|c| match filter::classify(c.utc, now) {
            Validity::RejectEpoch1904 | Validity::RejectFuture => None,
            v => Some((c, v)),
        })
        .collect()
}

/// 多数派仲裁：best 为 P0 时，若存在高置信 filename 候选与某高置信 mtime 候选互证
/// （差 ≤ `CONFLICT_OVER_DAY_SECS`）且 filename 与 P0 差 > `MTIME_VS_P0_HINT_SECS`，
/// 返回该 filename 候选作为新 best。LowConfidencePre1995 的两票互证不足以撼动 P0
/// （pre-1995 的 filename + mtime 常见于扫描件被批量 touch，置信度低不应推翻可信 P0）。
fn majority_override(
    best: &Candidate,
    others: &[(Candidate, Validity)],
) -> Option<(Candidate, Validity)> {
    if best.priority() != Priority::P0 {
        return None;
    }
    others
        .iter()
        .find(|(f, v)| {
            is_filename_source(f.source)
                && matches!(v, Validity::Valid)
                && (best.utc - f.utc).num_seconds().abs() > MTIME_VS_P0_HINT_SECS
                && others.iter().any(|(m, mv)| {
                    m.source == Source::FsMtime
                        && matches!(mv, Validity::Valid)
                        && (f.utc - m.utc).num_seconds().abs() <= CONFLICT_OVER_DAY_SECS
                })
        })
        .copied()
}

fn detect_conflicts(
    best: &Candidate,
    others: &[(Candidate, Validity)],
    gps_utc: Option<DateTime<Utc>>,
) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    if let Some(gps) = gps_utc {
        let diff = (best.utc - gps).num_seconds();
        if diff.abs() > CONFLICT_OVER_DAY_SECS {
            conflicts.push(Conflict {
                kind: ConflictKind::GpsOver24h,
                other_utc: gps,
                other_source: None,
                diff_secs: diff,
            });
        }
    }

    for (cand, _) in others {
        if is_filename_source(cand.source) {
            let diff = (best.utc - cand.utc).num_seconds();
            if diff.abs() > CONFLICT_OVER_DAY_SECS {
                conflicts.push(Conflict {
                    kind: ConflictKind::FilenameOver1Day,
                    other_utc: cand.utc,
                    other_source: Some(cand.source),
                    diff_secs: diff,
                });
            }
        }
        if cand.source == Source::FsMtime {
            let diff = (best.utc - cand.utc).num_seconds();
            // "mtime < P0 但差距较大" 仅提示。这里要求 mtime 严格早于 best。
            if diff > MTIME_VS_P0_HINT_SECS {
                conflicts.push(Conflict {
                    kind: ConflictKind::MtimeMuchEarlierThanP0,
                    other_utc: cand.utc,
                    other_source: Some(cand.source),
                    diff_secs: diff,
                });
            }
        }
    }

    conflicts
}

// 从 Priority 推导而非穷举 9 个 Filename* variant：与 priority.rs 单点同源，
// 新增 P2 来源时无需双写（漏改穷举会让新来源的冲突检测静默失效）。
fn is_filename_source(s: Source) -> bool {
    s.priority() == Priority::P2
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0).unwrap()
    }

    fn cand(source: Source, secs: i64) -> Candidate {
        Candidate {
            utc: Utc.timestamp_opt(secs, 0).single().unwrap(),
            offset: None,
            source,
            inferred_offset: false,
        }
    }

    #[test]
    fn empty_returns_none() {
        assert!(resolve(vec![], None, now()).is_none());
    }

    #[test]
    fn p0_wins_over_p1() {
        let d = resolve(
            vec![
                cand(Source::ExifCreateDate, 1_700_000_100), // P1 更早
                cand(Source::ExifDateTimeOriginal, 1_700_000_200), // P0 更晚但优先级更高
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.priority, Priority::P0);
        assert_eq!(d.utc.timestamp(), 1_700_000_200);
    }

    #[test]
    fn same_priority_takes_earlier() {
        // 两个 P0 候选，取更早
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, 1_700_000_200),
                cand(Source::QuickTimeCreationDate, 1_700_000_100),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.utc.timestamp(), 1_700_000_100);
    }

    #[test]
    fn epoch_1904_filtered_out() {
        let epoch = filter::quicktime_epoch().timestamp();
        let d = resolve(
            vec![
                cand(Source::QuickTimeCreateDate, epoch),
                cand(Source::ExifCreateDate, 1_700_000_100),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.utc.timestamp(), 1_700_000_100);
    }

    #[test]
    fn future_filtered_out() {
        let future = now().timestamp() + 100 * 86_400;
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, future),
                cand(Source::ExifCreateDate, 1_700_000_100),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.priority, Priority::P1);
    }

    #[test]
    fn pre_1995_kept_with_low_confidence() {
        // 1980-01-01
        let pre = 315_532_800;
        let d = resolve(vec![cand(Source::ExifDateTimeOriginal, pre)], None, now()).unwrap();
        assert_eq!(d.confidence, Confidence::Low);
    }

    #[test]
    fn confidence_high_when_no_pre_1995() {
        let d = resolve(
            vec![cand(Source::ExifDateTimeOriginal, 1_700_000_100)],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.confidence, Confidence::High);
    }

    #[test]
    fn gps_diff_over_24h_recorded_as_conflict() {
        let p0 = 1_700_000_100;
        let gps = Utc.timestamp_opt(p0 + 48 * 3600, 0).single().unwrap();
        let d = resolve(
            vec![cand(Source::ExifDateTimeOriginal, p0)],
            Some(gps),
            now(),
        )
        .unwrap();
        assert_eq!(d.conflicts.len(), 1);
        assert_eq!(d.conflicts[0].kind, ConflictKind::GpsOver24h);
    }

    #[test]
    fn gps_diff_within_24h_no_conflict() {
        let p0 = 1_700_000_100;
        let gps = Utc.timestamp_opt(p0 + 3600, 0).single().unwrap();
        let d = resolve(
            vec![cand(Source::ExifDateTimeOriginal, p0)],
            Some(gps),
            now(),
        )
        .unwrap();
        assert!(d.conflicts.is_empty());
    }

    #[test]
    fn filename_diff_over_one_day_recorded() {
        let p0 = 1_700_000_100;
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, p0),
                cand(Source::FilenamePhone, p0 + 2 * 86_400),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.conflicts.len(), 1);
        assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
    }

    #[test]
    fn mtime_much_earlier_than_p0_recorded() {
        let p0 = 1_700_000_100;
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, p0),
                cand(Source::FsMtime, p0 - 60 * 86_400),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.conflicts.len(), 1);
        assert_eq!(d.conflicts[0].kind, ConflictKind::MtimeMuchEarlierThanP0);
    }

    #[test]
    fn mtime_later_than_p0_not_recorded() {
        let p0 = 1_700_000_100;
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, p0),
                cand(Source::FsMtime, p0 + 60 * 86_400),
            ],
            None,
            now(),
        )
        .unwrap();
        // mtime > p0 → 不是"假象"模式（仅 mtime < P0 提示）
        assert!(d.conflicts.is_empty());
    }

    #[test]
    fn non_p0_best_skips_conflict_detection() {
        // 交叉校验仅针对 P0 best
        let d = resolve(
            vec![
                cand(Source::FilenamePhone, 1_700_000_100),
                cand(Source::FsMtime, 1_700_000_100 - 60 * 86_400),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.priority, Priority::P2);
        assert!(d.conflicts.is_empty());
    }

    // ── 多数派仲裁：filename 与 mtime 互证（差≤1天）且都与 P0 差>30天 ──
    // 典型场景：相机时钟错误，事后重命名的文件名时间 + 文件 mtime 才是真实时间。

    #[test]
    fn majority_filename_mtime_overrules_wrong_p0() {
        let p0 = 1_000_000_000; // 远早于 filename/mtime
        let real = p0 + 600 * 86_400;
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, p0),
                cand(Source::FilenameDashedDateTime, real),
                cand(Source::FsMtime, real + 3600),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.utc.timestamp(), real);
        assert_eq!(d.priority, Priority::P2);
        assert_eq!(d.source, Source::FilenameDashedDateTime);
        // 推翻 P0 必须可观测：记录冲突供 create_time 告警
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
                // mtime 与 filename 差 3 天 > 1 天：不互证
                cand(Source::FsMtime, f + 3 * 86_400),
            ],
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
            now(),
        )
        .unwrap();
        assert_eq!(d.priority, Priority::P0);
        assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
    }

    #[test]
    fn p0_kept_when_filename_within_30_days() {
        let p0 = 1_700_000_100;
        let f = p0 + 10 * 86_400; // >1 天（记冲突）但 ≤30 天（不推翻）
        let d = resolve(
            vec![
                cand(Source::ExifDateTimeOriginal, p0),
                cand(Source::FilenameDashedDateTime, f),
                cand(Source::FsMtime, f),
            ],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.priority, Priority::P0);
        assert_eq!(d.utc.timestamp(), p0);
        assert_eq!(d.conflicts[0].kind, ConflictKind::FilenameOver1Day);
    }

    #[test]
    fn surviving_includes_low_confidence_path() {
        // 全部 pre-1995 → best 是 pre-1995，confidence = Low
        let d = resolve(
            vec![cand(Source::ExifDateTimeOriginal, 315_532_800)],
            None,
            now(),
        )
        .unwrap();
        assert_eq!(d.confidence, Confidence::Low);
    }

    #[test]
    fn offset_and_inferred_flag_propagate() {
        let east8 = FixedOffset::east_opt(8 * 3600).unwrap();
        let c = Candidate {
            utc: Utc.timestamp_opt(1_700_000_100, 0).single().unwrap(),
            offset: Some(east8),
            source: Source::FilenamePhone,
            inferred_offset: true,
        };
        let d = resolve(vec![c], None, now()).unwrap();
        assert_eq!(d.offset, Some(east8));
        assert!(d.inferred_offset);
    }
}
