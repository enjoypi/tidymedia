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

/// spec §6："mtime < P0 但差距较大" 中的"较大"采用 30 天，超过即作为提示性冲突。
const MTIME_VS_P0_HINT_SECS: i64 = 30 * 86_400;

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
    let (best, validity) = surviving[0];
    let confidence = match validity {
        Validity::LowConfidencePre1995 => Confidence::Low,
        _ => Confidence::High,
    };

    let conflicts = if best.priority() == Priority::P0 {
        detect_conflicts(&best, &surviving[1..], gps_utc)
    } else {
        Vec::new()
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

fn apply_filters(
    candidates: Vec<Candidate>,
    now: DateTime<Utc>,
) -> Vec<(Candidate, Validity)> {
    candidates
        .into_iter()
        .filter_map(|c| match filter::classify(c.utc, now) {
            Validity::RejectEpoch1904 | Validity::RejectFuture => None,
            v => Some((c, v)),
        })
        .collect()
}

fn detect_conflicts(
    best: &Candidate,
    others: &[(Candidate, Validity)],
    gps_utc: Option<DateTime<Utc>>,
) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    if let Some(gps) = gps_utc {
        let diff = (best.utc - gps).num_seconds();
        if diff.abs() > 24 * 3600 {
            conflicts.push(Conflict {
                kind: ConflictKind::GpsOver24h,
                other_utc: gps,
                other_source: None,
                diff_secs: diff,
            });
        }
    }

    for (cand, _) in others.iter() {
        if is_filename_source(cand.source) {
            let diff = (best.utc - cand.utc).num_seconds();
            if diff.abs() > 86_400 {
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
            // spec §6："mtime < P0 但差距较大" 仅提示。这里要求 mtime 严格早于 best。
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

fn is_filename_source(s: Source) -> bool {
    matches!(
        s,
        Source::FilenameCamera
            | Source::FilenamePhone
            | Source::FilenameScreenshot
            | Source::FilenameUnixMillis
    )
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
        // mtime > p0 → 不是"假象"模式（spec §6 only mtime < P0 提示）
        assert!(d.conflicts.is_empty());
    }

    #[test]
    fn non_p0_best_skips_conflict_detection() {
        // spec §6 的交叉校验仅针对 P0 best
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
