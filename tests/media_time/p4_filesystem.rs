// spec §2.P4 / §5.10：mtime 兜底；btime/ctime 不可用。

use tidymedia::media_time::fs_time::from_modified;
use tidymedia::media_time::{epoch_to_candidate, resolve, Priority, Source};

use super::common::{fixed_now, set_mtime, utc_offset};

/// spec §2.P4：from_modified 把 mtime 转 P4 候选。
#[test]
fn mtime_yields_p4() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.bin");
    std::fs::write(&path, b"x").unwrap();
    set_mtime(&path, 1_714_545_000);
    let meta = std::fs::metadata(&path).unwrap();

    let c = from_modified(meta.modified().ok()).unwrap();
    assert_eq!(c.source, Source::FsMtime);
    assert_eq!(c.utc.timestamp(), 1_714_545_000);
}

/// spec §3 + §2.P4：resolve 把 P4 候选放在所有 P0-P3 之后。
#[test]
fn p4_lowest_priority() {
    let p4 = epoch_to_candidate(1_714_545_000, Source::FsMtime, None, false).unwrap();
    let p0 = epoch_to_candidate(
        1_700_000_000,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![p4, p0], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.utc.timestamp(), 1_700_000_000);
}

/// spec §5.10："mtime 看似可靠的假象" — 只有 mtime 时 P4 仍被采纳。
#[test]
fn mtime_only_picked_when_nothing_else() {
    let p4 = epoch_to_candidate(1_714_545_000, Source::FsMtime, None, false).unwrap();
    let d = resolve(vec![p4], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P4);
    assert_eq!(d.utc.timestamp(), 1_714_545_000);
}

/// spec §2.P4：metadata.modified() 不可用时返回 None，resolve 不应崩。
#[test]
fn from_modified_none_returns_none() {
    assert!(from_modified(None).is_none());
}
