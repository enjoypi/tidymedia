use std::fmt;
use std::fs;

use tempfile::tempdir;

use super::super::file_info;
use super::super::test_common as common;
use super::DuplicateGroup;
use super::Index;
use super::Info;

// 测试辅助：从 Vec<DuplicateGroup> 中按 size 查首个匹配组（旧 BTreeMap 索引语义的替代）。
fn group_by_size(groups: &[DuplicateGroup], size: u64) -> &DuplicateGroup {
    groups
        .iter()
        .find(|g| g.size == size)
        .expect("no duplicate group with the given size")
}

#[test]
fn insert() {
    let mut index = Index::new();
    let info = index.insert(common::DATA_SMALL).unwrap();
    let want = file_info::full_path(common::DATA_SMALL).unwrap();
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
fn parse_exif() {
    let mut index = Index::new();
    index.visit_dir(common::DATA_DIR);
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap());

    let jpeg_path = file_info::full_path(common::DATA_JPEG_WITH_EXIF).unwrap();
    let info = index.files.get(jpeg_path.as_path()).unwrap();
    let exif = info.exif().unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert!(exif.is_media());
    // 本 fixture EXIF 含 DateTimeOriginal=2024-01-01
    assert_eq!(exif.date_time_original(), 1_704_110_400);
}

#[test]
fn exists_returns_none_for_unrelated() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let other = Info::from(common::DATA_LARGE).unwrap();
    assert!(index.exists(&other, false).unwrap().is_none());
}

#[test]
fn exists_returns_some_for_duplicate() {
    let mut index = Index::new();
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

    let mut index = Index::new();
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
    assert_eq!(index.files().len(), 0);
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
    assert_eq!(index.files().len(), 1);
    let only = index.files().values().next().unwrap();
    assert!(only.full_path.as_str().ends_with("real.bin"));
}

#[test]
fn add_idempotent_on_same_path() {
    let mut index = Index::new();
    let first = Info::from(common::DATA_SMALL).unwrap();
    let key = first.full_path.clone();
    index.add(first);
    let again = Info::from(common::DATA_SMALL).unwrap();
    index.add(again);
    assert_eq!(index.files().len(), 1);
    assert!(index.files().contains_key(&key));
}

#[test]
fn some_files_sorts_and_limits() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_LARGE).unwrap();
    index.insert(common::DATA_DNS_BENCHMARK).unwrap();
    let two = index.some_files(2);
    assert_eq!(two.len(), 2);
    assert!(two[0].full_path <= two[1].full_path);
}

#[test]
fn bytes_read_sums_individual() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_LARGE).unwrap();
    let total: u64 = index
        .files()
        .values()
        .map(super::super::file_info::Info::bytes_read)
        .sum();
    assert_eq!(index.bytes_read(), total);
}

#[test]
fn parse_exif_empty_index_ok() {
    let mut index = Index::new();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap());
    assert_eq!(index.files().len(), 0);
}

/// 文件在 `visit_dir` 之后被删除 → `Exif::from_path` 返回 Err →
/// `parse_exif` 内 `if let Ok` 的 Err 分支被覆盖，对应 entry 保留无 exif。
#[test]
fn parse_exif_skips_files_deleted_between_visit_and_parse() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ghost.bin");
    fs::write(&path, [0xFFu8; 32]).unwrap();
    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());
    assert_eq!(index.files().len(), 1);
    fs::remove_file(&path).unwrap();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap());
    assert_eq!(index.files().len(), 1);
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
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let dbg = format!("{index:?}");
    assert!(dbg.contains("data_small"));
}

// 自定义 fmt::Write 总返回 Err 强制 Debug 实现里的 writeln!(...)? 走 Err 分支。
// 覆盖 file_index.rs L25 的 ? Err region。
struct FailingWriter;
impl fmt::Write for FailingWriter {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[test]
fn debug_fmt_propagates_writer_error() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let res = fmt::write(&mut FailingWriter, format_args!("{index:?}"));
    assert!(res.is_err());
}

// 外部传入的 src_file 底层已删除，让 exists() 中
// `f.calc_full_hash()? == src_file.calc_full_hash()?` 的右侧 ? 走 Err 分支。
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

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    // 仅删 src 文件 b，保留 index 中的 a
    fs::remove_file(&b_path).unwrap();
    let err = index.exists(&info_b, false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// index 中保存的 Info 指向的源文件被外部删除后，exists() 内调用 calc_full_hash
// 会因 mmap 失败而 Err，触发 L70 的 ? Err 分支。
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

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();

    // 删 index 中已经登记的 a 文件
    fs::remove_file(&a_path).unwrap();
    let err = index.exists(&info_b, false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// 触发 insert() 内 Info::from(path)? 的 Err 分支（L150）。
#[test]
fn insert_propagates_info_from_error() {
    let mut index = Index::new();
    let err = index.insert("/nonexistent/zzz999").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// calc_same 预过滤只放行 fast_hash 冲突桶（len>1）：唯一文件的单例桶不得进入
// 逐文件 hash 阶段。结果以 pub calc_same 返回 Vec 的元素个数观测（search_same 的
// filter_and_sort 会把单例再滤掉，杀不了「>1 变 >=1」边界变异）。
#[test]
fn calc_same_excludes_singleton_buckets_from_results() {
    let dir = tempdir().unwrap();
    let dup_a = dir.path().join("dup_a.bin");
    fs::write(&dup_a, b"same-content-same-content").unwrap();
    let dup_b = dir.path().join("dup_b.bin");
    fs::write(&dup_b, b"same-content-same-content").unwrap();
    let unique = dir.path().join("unique.bin");
    fs::write(&unique, b"totally-different-payload!").unwrap();

    let mut index = Index::new();
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

// 让 similar_files 有两个冲突 path，其中一个对应的文件已删除：
// - calc(info) 对删除的文件 Err → 被 `if let Ok` 过滤 (L98 失败分支)
// - 剩 1 个 path → filter_and_sort 走 paths.len()==1 的 else 分支 (L126)
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

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    index.insert(b_path.to_str().unwrap()).unwrap();

    // 删除 a，让 secure_hash 在 calc_same 中对 a Err
    fs::remove_file(&a_path).unwrap();

    let same = index.search_same();
    // a 被 calc Err 过滤掉；b 剩单独一条，paths.len()==1，filter_and_sort 不保留
    assert!(same.is_empty());
}
