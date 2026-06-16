// docs/media-time-detection.md §三 + §六：合并 + 冲突校验。
//   - 按 Priority 升序排序（P0 优先），同优先级取较早 utc
//   - 1904 / 未来时间过滤，1995 之前降置信
//   - 当 best.priority == P0 时做交叉校验：GPS、filename、mtime
//   - 多数派仲裁可被 EXIF ModifyDate 三方互证否决（re-save 痕迹识别）

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
    modify_date_utc: Option<DateTime<Utc>>,
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
    match majority_verdict(&best, &surviving[1..], modify_date_utc) {
        MajorityVerdict::Override(winner, v) => {
            // diff_secs 约定 = chosen - other_utc（与 detect_conflicts 各分支一致）：
            // 这里 chosen=winner、other_utc=原 P0=best，故 winner.utc - best.utc。
            // 颠倒会让下游脚本（tidy-verify 等）按"diff = chosen - other"解读相机
            // 时钟偏差方向时正负相反。
            conflicts.push(Conflict {
                kind: ConflictKind::P0OverruledByMajority,
                other_utc: best.utc,
                other_source: Some(best.source),
                diff_secs: (winner.utc - best.utc).num_seconds(),
            });
            (best, validity) = (winner, v);
        }
        MajorityVerdict::Vetoed(loser) => {
            // 否决可观测：记被否决的 filename 票，再走常规冲突检测
            //（filename 与 P0 差>30天必然附带 FilenameOver1Day 提示）。
            conflicts.push(Conflict {
                kind: ConflictKind::MajorityVetoedByModifyDate,
                other_utc: loser.utc,
                other_source: Some(loser.source),
                diff_secs: (best.utc - loser.utc).num_seconds(),
            });
            conflicts.extend(detect_conflicts(&best, &surviving[1..], gps_utc));
        }
        MajorityVerdict::NoQuorum => {
            if best.priority() == Priority::P0 {
                conflicts = detect_conflicts(&best, &surviving[1..], gps_utc);
            }
        }
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

/// 多数派仲裁结论。`Vetoed` 仅在互证成立但被 `ModifyDate` 否决时出现，
/// 携带被否决的 filename 票供调用方记冲突（不静默）。
enum MajorityVerdict {
    Override(Candidate, Validity),
    Vetoed(Candidate),
    NoQuorum,
}

/// 多数派仲裁：best 为 P0 时，若存在高置信 filename 候选与某高置信 mtime 候选互证
/// （差 ≤ `CONFLICT_OVER_DAY_SECS`）且 filename 与 P0 差 > `MTIME_VS_P0_HINT_SECS`，
/// 该 filename 候选成为新 best。LowConfidencePre1995 的两票互证不足以撼动 P0
/// （pre-1995 的 filename + mtime 常见于扫描件被批量 touch，置信度低不应推翻可信 P0）。
///
/// `ModifyDate` 否决：互证成立但 filename 票与 EXIF `ModifyDate` 差 ≤ 1 天时，
/// filename+mtime+`ModifyDate` 三方吻合说明三者都是 re-save 时戳（第三方批量
/// re-save 保留 DTO、刷新 `ModifyDate`+mtime 并按 re-save 时间命名的典型痕迹），
/// 不构成推翻 P0 的证据。无需再比 `ModifyDate` 与 P0 的距离——filename 与 P0
/// 差>30天是互证前提，filename≈`ModifyDate` 已蕴含 `ModifyDate` 远离 P0。
fn majority_verdict(
    best: &Candidate,
    others: &[(Candidate, Validity)],
    modify_date_utc: Option<DateTime<Utc>>,
) -> MajorityVerdict {
    if best.priority() != Priority::P0 {
        return MajorityVerdict::NoQuorum;
    }
    let quorum = others.iter().find(|(f, v)| {
        is_filename_source(f.source)
            && matches!(v, Validity::Valid)
            && (best.utc - f.utc).num_seconds().abs() > MTIME_VS_P0_HINT_SECS
            && others.iter().any(|(m, mv)| {
                m.source == Source::FsMtime
                    && matches!(mv, Validity::Valid)
                    && (f.utc - m.utc).num_seconds().abs() <= CONFLICT_OVER_DAY_SECS
            })
    });
    match quorum {
        Some(&(winner, v)) => {
            let vetoed = modify_date_utc
                .is_some_and(|md| (winner.utc - md).num_seconds().abs() <= CONFLICT_OVER_DAY_SECS);
            if vetoed {
                MajorityVerdict::Vetoed(winner)
            } else {
                MajorityVerdict::Override(winner, v)
            }
        }
        None => MajorityVerdict::NoQuorum,
    }
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
#[path = "resolve_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "resolve_basic_tests.rs"]
mod basic_tests;

#[cfg(test)]
#[path = "resolve_majority_tests.rs"]
mod majority_tests;
