use std::fmt;
use std::fs;

use tempfile::tempdir;

use super::super::file_info;
use super::super::test_common as common;
use super::DuplicateGroup;
use super::Index;
use super::Info;

fn group_by_size(groups: &[DuplicateGroup], size: u64) -> &DuplicateGroup {
    groups
        .iter()
        .find(|g| g.size == size)
        .expect("no duplicate group with the given size")
}

#[test]
fn insert() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let want = file_info::full_path(common::DATA_SMALL).unwrap();
    let entry = index
        .get(want.as_path())
        .expect("insert must land in files map");
    let info = entry.value();
    assert_eq!(info.full_path, want);
    assert_eq!(info.fast_hash, common::DATA_SMALL_WYHASH);
    assert_eq!(info.calc_full_hash().unwrap(), common::DATA_SMALL_XXHASH);
    assert_eq!(info.secure_hash().unwrap(), common::data_small_sha512());
}

#[test]
fn search_same() {
    let mut index = Index::new();
    index.visit_dir(common::DATA_DIR);

    let same = index.search_same();
    assert_eq!(same.len(), 2);
    let large = group_by_size(&same, common::DATA_LARGE_LEN);
    let small = group_by_size(&same, common::DATA_SMALL_LEN);
    assert_eq!(large.paths.len(), 2);
    assert_eq!(small.paths.len(), 2);

    let large_path = file_info::full_path(common::DATA_LARGE).unwrap();
    let large_copy = file_info::full_path(common::DATA_LARGE_COPY).unwrap();
    let small_path = file_info::full_path(common::DATA_SMALL).unwrap();
    let small_copy = file_info::full_path(common::DATA_SMALL_COPY).unwrap();
    assert!(large.paths.contains(&large_path));
    assert!(large.paths.contains(&large_copy));
    assert!(small.paths.contains(&small_path));
    assert!(small.paths.contains(&small_copy));
}

#[test]
fn exists_returns_none_for_unrelated() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let other = Info::from(common::DATA_LARGE).unwrap();
    assert!(index.exists(&other, false).unwrap().is_none());
}

#[test]
fn exists_returns_some_for_duplicate() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let copy = Info::from(common::DATA_SMALL_COPY).unwrap();
    let found = index
        .exists(&copy, false)
        .unwrap()
        .expect("duplicate must be detected");
    assert_eq!(found, file_info::full_path(common::DATA_SMALL).unwrap());
}

#[test]
fn exists_handles_fast_hash_collision_with_different_content() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();

    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    let info_a_ref = Info::from(a_path.to_str().unwrap()).unwrap();
    assert_eq!(info_a_ref.fast_hash, info_b.fast_hash);
    assert!(index.exists(&info_b, false).unwrap().is_none());
}

#[test]
fn visit_dir_handles_nonexistent_path() {
    let mut index = Index::new();
    index.visit_dir("/no/such/directory/xyz123");
    assert_eq!(index.len(), 0);
}

#[test]
fn visit_dir_falls_back_when_relative_path_cannot_canonicalize() {
    let mut index = Index::new();
    index.visit_dir("relative/path/that/does/not/exist/xyz123");
    assert_eq!(index.len(), 0);
}

#[test]
fn visit_dir_skips_empty_files() {
    let dir = tempdir().unwrap();
    let empty_path = dir.path().join("empty.bin");
    fs::write(&empty_path, b"").unwrap();
    let real_path = dir.path().join("real.bin");
    fs::write(&real_path, b"abcdef").unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());
    assert_eq!(index.len(), 1);
    let entry = index.iter().next().unwrap();
    let only = entry.value();
    assert!(only.full_path.as_str().ends_with("real.bin"));
}

#[test]
fn add_idempotent_on_same_path() {
    let index = Index::new();
    let first = Info::from(common::DATA_SMALL).unwrap();
    let key = first.full_path.clone();
    index.add(first);
    let again = Info::from(common::DATA_SMALL).unwrap();
    index.add(again);
    assert_eq!(index.len(), 1);
    assert!(index.contains_key(key.as_path()));
}

#[test]
fn some_files_sorts_and_limits() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_LARGE).unwrap();
    index.insert(common::DATA_DNS_BENCHMARK).unwrap();
    let two = index.some_files(2);
    assert_eq!(two.len(), 2);
    assert!(two[0] <= two[1]);
}

#[test]
fn bytes_read_sums_individual() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_LARGE).unwrap();
    let total: u64 = index.iter().map(|kv| kv.value().bytes_read()).sum();
    assert_eq!(index.bytes_read(), total);
}

#[test]
fn fast_search_same_matches_search_same() {
    let mut index = Index::new();
    index.visit_dir(common::DATA_DIR);
    let secure = index.search_same();
    let fast = index.fast_search_same();
    assert_eq!(secure, fast);
}

#[test]
fn index_debug_format_renders_files() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let dbg = format!("{index:?}");
    assert!(dbg.contains("data_small"));
}

struct FailingWriter;
impl fmt::Write for FailingWriter {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[test]
fn debug_fmt_propagates_writer_error() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let res = fmt::write(&mut FailingWriter, format_args!("{index:?}"));
    assert!(res.is_err());
}

#[test]
fn exists_propagates_calc_hash_error_when_src_deleted() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    fs::remove_file(&b_path).unwrap();
    let err = index.exists(&info_b, false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn exists_propagates_calc_hash_error_when_file_deleted() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    fs::remove_file(&a_path).unwrap();
    let err = index.exists(&info_b, false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn insert_propagates_info_from_error() {
    let index = Index::new();
    let err = index.insert("/nonexistent/zzz999").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn calc_same_excludes_singleton_buckets_from_results() {
    let dir = tempdir().unwrap();
    let dup_a = dir.path().join("dup_a.bin");
    fs::write(&dup_a, b"same-content-same-content").unwrap();
    let dup_b = dir.path().join("dup_b.bin");
    fs::write(&dup_b, b"same-content-same-content").unwrap();
    let unique = dir.path().join("unique.bin");
    fs::write(&unique, b"totally-different-payload!").unwrap();

    let index = Index::new();
    index.insert(dup_a.to_str().unwrap()).unwrap();
    index.insert(dup_b.to_str().unwrap()).unwrap();
    index.insert(unique.to_str().unwrap()).unwrap();

    let results = index.calc_same(Info::secure_hash);
    assert_eq!(
        results.len(),
        1,
        "only the duplicate fast-hash bucket may enter the calc phase"
    );
}

#[test]
fn remove_under_prefix_removes_subtree_and_keeps_outside() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("archive");
    fs::create_dir_all(&sub).unwrap();
    let outside = dir.path().join("a.bin");
    let inside = sub.join("b.bin");
    fs::write(&outside, b"same-content-x").unwrap();
    fs::write(&inside, b"same-content-x").unwrap();

    let index = Index::new();
    index.insert(outside.to_str().unwrap()).unwrap();
    index.insert(inside.to_str().unwrap()).unwrap();
    assert_eq!(index.len(), 2);

    let sub_prefix = file_info::full_path(sub.to_str().unwrap()).unwrap();
    let removed = index.remove_under_prefix(sub_prefix.as_str());
    assert_eq!(removed, 1);
    assert_eq!(index.len(), 1);

    let probe = Info::from(outside.to_str().unwrap()).unwrap();
    let dup = index.exists(&probe, false).unwrap();
    assert!(dup.is_some(), "outside file must remain queryable");
}

#[test]
fn remove_under_prefix_does_not_match_sibling_with_common_prefix() {
    let dir = tempdir().unwrap();
    let backup = dir.path().join("photos_backup");
    fs::create_dir_all(&backup).unwrap();
    let f = backup.join("a.bin");
    fs::write(&f, b"data-1234").unwrap();

    let index = Index::new();
    index.insert(f.to_str().unwrap()).unwrap();

    let photos_prefix = dir.path().join("photos");
    let removed = index.remove_under_prefix(photos_prefix.to_str().unwrap());
    assert_eq!(removed, 0);
    assert_eq!(index.len(), 1);
}

#[test]
fn calc_same_skips_files_with_calc_error_and_singletons() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    index.insert(b_path.to_str().unwrap()).unwrap();

    fs::remove_file(&a_path).unwrap();

    let same = index.search_same();
    assert_eq!(same, []);
}

#[test]
fn into_infos_returns_every_indexed_file() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let infos = index.into_infos();
    assert_eq!(infos.len(), 1);
    assert_eq!(
        infos[0].full_path,
        file_info::full_path(common::DATA_SMALL).unwrap()
    );
}

#[test]
fn visit_stats_skipped_total_sums_four_skip_categories() {
    let stats = super::VisitStats {
        skipped_empty: 1,
        skipped_unreadable: 2,
        walker_errors: 3,
        skipped_by_ext: 4,
    };
    assert_eq!(stats.skipped_total(), 10);
    assert_eq!(super::VisitStats::default().skipped_total(), 0);
}

#[test]
fn touched_count_equals_indexed_plus_all_skip_categories() {
    let mut index = Index::new();
    assert_eq!(index.touched_count(), 0);
    index.insert(common::DATA_SMALL).unwrap();
    index.stats.skipped_empty = 1;
    index.stats.skipped_unreadable = 1;
    index.stats.walker_errors = 1;
    index.stats.skipped_by_ext = 1;
    assert_eq!(index.touched_count(), 5);
}
