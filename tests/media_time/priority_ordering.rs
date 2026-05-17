// spec §三：优先级表 + 同优先级取较早。

use tidymedia::media_time::{epoch_to_candidate, resolve, Priority, Source};

use super::common::{fixed_now, utc_offset};

/// spec §3：P0 > P1 > P2 > P3 > P4。
#[test]
fn priority_descends_p0_to_p4() {
    let cs = vec![
        epoch_to_candidate(1_700_000_005, Source::FsMtime, None, false).unwrap(),
        epoch_to_candidate(1_700_000_004, Source::XmpSidecar, None, false).unwrap(),
        epoch_to_candidate(
            1_700_000_003,
            Source::FilenamePhone,
            Some(utc_offset()),
            true,
        )
        .unwrap(),
        epoch_to_candidate(
            1_700_000_002,
            Source::ExifCreateDate,
            Some(utc_offset()),
            false,
        )
        .unwrap(),
        epoch_to_candidate(
            1_700_000_001,
            Source::ExifDateTimeOriginal,
            Some(utc_offset()),
            false,
        )
        .unwrap(),
    ];
    let d = resolve(cs, None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.source, Source::ExifDateTimeOriginal);
}

/// spec §3："同优先级冲突时取较早的值（更接近原始拍摄）"。
#[test]
fn same_priority_takes_earlier() {
    let later = epoch_to_candidate(
        1_700_000_200,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let earlier = epoch_to_candidate(
        1_700_000_100,
        Source::QuickTimeCreationDate,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![later, earlier], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), 1_700_000_100);
}

/// spec §3：候选全空 → resolve 返回 None。
#[test]
fn no_candidates_resolves_to_none() {
    assert!(resolve(vec![], None, fixed_now()).is_none());
}
