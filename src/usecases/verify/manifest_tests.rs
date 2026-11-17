use std::fs;
use std::io;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::Index;
use crate::entities::uri::Location;

fn loc(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn file_entry(path: &str, size: u64) -> Entry {
    Entry {
        location: loc(path),
        size,
        kind: EntryKind::File,
    }
}

fn dir_entry(path: &str) -> Entry {
    Entry {
        location: loc(path),
        size: 0,
        kind: EntryKind::Dir,
    }
}

fn collected(stream: &mut EntryStream<'_>) -> Vec<io::Result<Entry>> {
    stream.by_ref().collect()
}

fn seeded_backend(files: &[(&str, usize)]) -> (tempfile::TempDir, std::sync::Arc<dyn Backend>) {
    let dir = tempdir().expect("tempdir");
    for (name, len) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, vec![b'x'; *len]).expect("write");
    }
    (dir, LocalBackend::arc())
}

fn rows_of(entries: Vec<io::Result<Entry>>) -> Vec<(String, u64)> {
    entries
        .into_iter()
        .map(|entry| entry.expect("entry"))
        .filter(|entry| entry.kind == EntryKind::File)
        .map(|entry| (entry.location.display(), entry.size))
        .collect()
}

#[test]
fn off_cache_streams_live_and_publishes_nothing() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3), ("b.jpg", 4)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    let cache = ScanCache::off();
    assert!(matches!(cache.entries(&root, &be), EntryStream::Live(_)));
    let first = rows_of(collected(&mut cache.entries(&root, &be)));
    let second = rows_of(collected(&mut cache.entries(&root, &be)));
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
}

#[test]
fn recorded_manifest_replays_identical_entries() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3), ("子 目录/带 空格.jpg", 7), ("中文.jpg", 5)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    with_manifest_min_entries(1, || {
        let cache = ScanCache::on();
        let live = rows_of(collected(&mut cache.entries(&root, &be)));
        assert_eq!(live.len(), 3);
        assert!(matches!(
            cache.entries(&root, &be),
            EntryStream::Replay { .. }
        ));
        let replayed = rows_of(collected(&mut cache.entries(&root, &be)));
        assert_eq!(replayed, live);
    });
}

#[test]
fn below_threshold_publishes_no_manifest() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    with_manifest_min_entries(u64::MAX, || {
        let cache = ScanCache::on();
        let streamed = rows_of(collected(&mut cache.entries(&root, &be)));
        assert_eq!(streamed.len(), 1);
        assert!(matches!(cache.entries(&root, &be), EntryStream::Live(_)));
    });
}

#[test]
fn abandoned_stream_discards_partial_manifest() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3), ("b.jpg", 4), ("c.jpg", 5)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    with_manifest_min_entries(1, || {
        let cache = ScanCache::on();
        let mut stream = cache.entries(&root, &be);
        assert!(stream.next().is_some());
        drop(stream);
        assert!(matches!(cache.entries(&root, &be), EntryStream::Live(_)));
    });
}

#[test]
fn concurrent_stream_while_recording_walks_live() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3), ("b.jpg", 4)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    with_manifest_min_entries(1, || {
        let cache = ScanCache::on();
        let mut recording = cache.entries(&root, &be);
        assert!(matches!(cache.entries(&root, &be), EntryStream::Live(_)));
        assert_eq!(rows_of(collected(&mut recording)).len(), 2);
        assert!(matches!(
            cache.entries(&root, &be),
            EntryStream::Replay { .. }
        ));
    });
}

#[test]
fn row_of_records_only_plain_files() {
    assert_eq!(
        Row::of(&Ok(file_entry("D:/old/a.jpg", 9))),
        Some(Row {
            path: "D:/old/a.jpg".to_owned(),
            size: 9
        })
    );
    assert_eq!(Row::of(&Ok(dir_entry("D:/old/sub"))), None);
    assert_eq!(
        Row::of(&Err(io::Error::new(
            io::ErrorKind::NotFound,
            "walker error"
        ))),
        None
    );
}

#[test]
fn rows_rebuild_location_from_root_scheme() {
    let local = Row {
        path: "D:/old/照片 2024/IMG 0001.jpg".to_owned(),
        size: 4,
    };
    let entry = local.entry(&loc("D:/old")).expect("local row");
    assert_eq!(entry.location, loc("D:/old/照片 2024/IMG 0001.jpg"));
    assert_eq!(entry.size, 4);
    assert_eq!(entry.kind, EntryKind::File);
}

#[test]
fn local_path_holding_scheme_marker_stays_local() {
    let row = Row {
        path: "D:/a://b/c.jpg".to_owned(),
        size: 1,
    };
    let entry = row
        .entry(&loc("D:/a"))
        .expect("local root must not parse scheme");
    assert_eq!(entry.location, loc("D:/a://b/c.jpg"));
}

#[test]
fn smb_rows_round_trip_and_reject_garbage() {
    let root = Location::Smb {
        user: None,
        host: "srv".to_owned(),
        port: Some(445),
        share: "photo".to_owned(),
        path: Utf8PathBuf::from("2024"),
    };
    let row = Row {
        path: root.display(),
        size: 6,
    };
    let entry = row.entry(&root).expect("smb row");
    assert_eq!(entry.location, root);
    assert_eq!(entry.size, 6);
    let broken = Row {
        path: "nfs://srv/share/x.jpg".to_owned(),
        size: 6,
    };
    assert!(broken.entry(&root).is_err());
}

#[test]
fn replay_surfaces_decode_error_and_end_of_stream() {
    let root = loc("D:/old");
    let mut broken = EntryStream::Replay(Replay {
        reader: manifest_io::reader_from("not json\n", 1),
        root: root.clone(),
    });
    assert!(matches!(collected(&mut broken).first(), Some(Err(_))));
    let mut short = EntryStream::Replay(Replay {
        reader: manifest_io::reader_from("", 3),
        root,
    });
    assert!(collected(&mut short).is_empty());
}

#[test]
fn replay_counts_open_failure_once_and_keeps_scanned_rows() {
    let (dir, be) = seeded_backend(&[("a.jpg", 3), ("b.jpg", 4)]);
    let root = loc(dir.path().to_str().expect("utf8"));
    with_manifest_min_entries(1, || {
        let cache = ScanCache::on();
        assert_eq!(rows_of(collected(&mut cache.entries(&root, &be))).len(), 2);
        fs::remove_file(dir.path().join("a.jpg")).expect("remove");
        let mut index = Index::new();
        let mut stream = cache.entries(&root, &be);
        index.visit_location_from(&root, &be, &mut stream, &[]);
        assert_eq!(index.len(), 1);
        assert_eq!(index.stats().skipped_unreadable, 1);
    });
}

#[test]
fn keeps_boundary_is_inclusive() {
    assert!(!keeps(9, 10));
    assert!(keeps(10, 10));
    assert!(keeps(0, 0));
}

#[test]
fn poisoned_slot_map_degrades_without_publish_or_cleanup() {
    let slots: Slots = Arc::new(Mutex::new(HashMap::new()));
    let poison = Arc::clone(&slots);
    let panicked = std::panic::catch_unwind(move || {
        let _guard = poison.lock().expect("fresh lock");
        panic!("internal: test poison");
    });
    assert!(panicked.is_err());
    let stream = |key: &str| Live {
        inner: Box::new(std::iter::empty::<io::Result<Entry>>()),
        rec: Some((
            manifest_io::Recorder::new().expect("recorder"),
            Arc::clone(&slots),
            key.to_owned(),
            1,
        )),
    };
    stream("sealed").seal();
    drop(stream("dropped"));
    assert!(slots.lock().is_err());
}
