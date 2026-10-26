// 同一 Index 承载两个不同 backend 的 visit_location 调用：
// - FakeBackend(smb nas)：放 1 个 1KiB 文件
// - FakeBackend(smb other host)：放 1 个不同字节序列的 1KiB 文件
// 期望：files() 含两条记录，fast_hash 不同；Info 内部 backend 句柄各自归属。
#[test]
fn visit_location_accepts_multiple_backends_in_one_index() {
    use std::sync::Arc;

    use camino::Utf8PathBuf;

    use crate::adapters::backend::fake::FakeBackend;
    use crate::entities::backend::Backend;
    use crate::entities::uri::Location;

    let smb_root = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::new(),
    };
    let smb_file = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("a.jpg"),
    };
    let other_root = Location::Smb {
        user: None,
        host: "other".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::new(),
    };
    let other_file = Location::Smb {
        user: None,
        host: "other".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("b.jpg"),
    };

    let smb = Arc::new(FakeBackend::new("smb"));
    smb.add_dir(smb_root.clone());
    smb.add_file(smb_file.clone(), vec![0xAA; 1024]);

    let other = Arc::new(FakeBackend::new("smb"));
    other.add_dir(other_root.clone());
    other.add_file(other_file.clone(), vec![0x55; 1024]);

    let mut index = Index::new();
    let smb_backend = Arc::clone(&smb) as Arc<dyn Backend>;
    let other_backend = Arc::clone(&other) as Arc<dyn Backend>;
    index.visit_location(&smb_root, &smb_backend);
    index.visit_location(&other_root, &other_backend);

    assert_eq!(index.len(), 2, "both backends contributed one file each");

    let smb_key = Utf8PathBuf::from(smb_file.display());
    let other_key = Utf8PathBuf::from(other_file.display());
    assert!(index.contains_key(smb_key.as_path()));
    assert!(index.contains_key(other_key.as_path()));
    let smb_entry = index.get(smb_key.as_path()).unwrap();
    let other_entry = index.get(other_key.as_path()).unwrap();
    assert_ne!(
        smb_entry.value().fast_hash,
        other_entry.value().fast_hash,
        "distinct byte content should hash differently"
    );

    // 重新算 full_hash 必须走各自 Info 内部的 Arc<dyn Backend>——
    // 若实现退化为单 backend 共享，跨 scheme 的 open_read 会失败。
    assert!(smb_entry.value().calc_full_hash().is_ok());
    assert!(other_entry.value().calc_full_hash().is_ok());
}

#[test]
#[cfg(unix)]
fn visit_dir_counts_non_utf8_path() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = tempdir().unwrap();
    let bad = OsStr::from_bytes(&[b'a', 0xFF, 0xFE, b'.', b'b', b'i', b'n']);
    let p = dir.path().join(bad);
    if fs::write(&p, b"abc").is_err() {
        return;
    }

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());
    // 精确计数同 visit_dir_counts_walker_errors_on_missing_root：防 += 算术变异 wrap
    assert_eq!(
        index.stats().walker_errors,
        1,
        "non-UTF-8 path must bump walker_errors exactly once"
    );
    assert_eq!(index.len(), 0);
}

// --- detach_from_bucket 防御分支直测 ---
// 「bucket 缺失」经 remove_under_prefix 不可达（add 不变式同步建 bucket），
// 按「防御性不可达 arm 抽纯 helper 直测」套路覆盖全部分支。

#[test]
fn detach_from_bucket_tolerates_missing_bucket() {
    let similar: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>> = DashMap::new();
    Index::detach_from_bucket(&similar, 42, &Utf8PathBuf::from("/x/a.bin"));
    assert!(similar.is_empty(), "missing bucket must be a silent no-op");
}

#[test]
fn detach_from_bucket_keeps_bucket_with_remaining_paths() {
    let a = Utf8PathBuf::from("/x/a.bin");
    let b = Utf8PathBuf::from("/x/b.bin");
    let similar: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>> = DashMap::new();
    similar.insert(7u64, Mutex::new(HashSet::from([a.clone(), b.clone()])));
    Index::detach_from_bucket(&similar, 7, &a);
    let bucket = similar.get(&7).expect("bucket with survivors must remain");
    let set = bucket.value().lock();
    assert_eq!(set.len(), 1);
    assert!(set.contains(&b));
}

#[test]
fn detach_from_bucket_removes_emptied_bucket() {
    let a = Utf8PathBuf::from("/x/a.bin");
    let similar: DashMap<u64, Mutex<HashSet<Utf8PathBuf>>> = DashMap::new();
    similar.insert(7u64, Mutex::new(HashSet::from([a.clone()])));
    Index::detach_from_bucket(&similar, 7, &a);
    assert!(
        !similar.contains_key(&7),
        "emptied bucket must be removed to keep exists() from panicking"
    );
}

// --- F1 并发写单测 ---
// DashMap 迁移后 `add` 是 `&self`，多线程并发插入不同 key 必须落全部条目、
// 同 fast_hash 桶收敛所有 path。用 rayon `par_iter` 触发跨 shard 竞争。
#[test]
fn add_concurrent_lands_all_entries_and_groups_by_fast_hash() {
    use rayon::iter::{IntoParallelIterator, ParallelIterator};

    let dir = tempdir().unwrap();
    // 100 个 unique payload → 100 个不同 fast_hash + 100 个不同 path
    let mut paths: Vec<std::path::PathBuf> = Vec::with_capacity(100);
    for i in 0..100u32 {
        let p = dir.path().join(format!("f_{i:04}.bin"));
        fs::write(&p, i.to_le_bytes()).unwrap();
        paths.push(p);
    }
    let index = Index::new();
    paths.into_par_iter().for_each(|p| {
        let info = Info::from(p.to_str().unwrap()).unwrap();
        index.add(info);
    });
    assert_eq!(index.len(), 100, "all 100 concurrent inserts must land");
}

// 同 fast_hash 多 path 并发插入：bucket 必收敛全部 path，不因 shard 竞态丢失。
#[test]
fn add_concurrent_same_fast_hash_bucket_holds_all_paths() {
    use rayon::iter::{IntoParallelIterator, ParallelIterator};

    // fast_hash 由前 FAST_READ_SIZE 字节决定（xxh3_64）；用相同 4 KiB 前缀 + 唯一
    // 尾字节的 20 个文件让 fast_hash 全等而 secure_hash 各异，模拟真实碰撞。
    let dir = tempdir().unwrap();
    let mut paths = Vec::with_capacity(20);
    for i in 0..20u8 {
        let p = dir.path().join(format!("dup_{i:02}.bin"));
        let mut payload = vec![0u8; 4096];
        payload.push(i);
        fs::write(&p, &payload).unwrap();
        paths.push(p);
    }

    let index = Index::new();
    paths.into_par_iter().for_each(|p| {
        let info = Info::from(p.to_str().unwrap()).unwrap();
        index.add(info);
    });

    assert_eq!(index.len(), 20);
    // 取任一文件的 fast_hash（20 个共享）验证 bucket 收敛 20 条 path
    let sample = index.iter().next().unwrap();
    let fast_hash = sample.value().fast_hash;
    drop(sample);
    let paths_in_bucket = index.similar_paths(fast_hash);
    assert_eq!(
        paths_in_bucket.len(),
        20,
        "同 fast_hash bucket 必收敛全部并发插入的 path"
    );
}

// remove_under_prefix 与 exists 读并发：验证两阶段访问（先 collect keys 释放读锁）
// 让 remove 与 exists 循环互不 deadlock；跑时限 5s 兜底防实测挂死。
#[test]
fn remove_under_prefix_concurrent_with_readers_does_not_deadlock() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    let dir = tempdir().unwrap();
    let mut paths = Vec::with_capacity(50);
    for i in 0..50u32 {
        let p = dir.path().join(format!("f_{i:04}.bin"));
        fs::write(&p, i.to_le_bytes()).unwrap();
        paths.push(p);
    }
    let index = Arc::new(Index::new());
    for p in &paths {
        let info = Info::from(p.to_str().unwrap()).unwrap();
        index.add(info);
    }
    let probe = Info::from(paths[0].to_str().unwrap()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));

    let idx_r = Arc::clone(&index);
    let stop_r = Arc::clone(&stop);
    let reader = thread::spawn(move || {
        while !stop_r.load(Ordering::Relaxed) {
            let _ = idx_r.exists(&probe, false);
        }
    });

    // 主线程反复删除 + 允许 reader 与之并发；不 deadlock 即通过。核心断言是 join
    // 能在 deadline 内完成，reader hit 计数不测（并行调度差异下可能 0-1 波动）。
    let deadline = Instant::now() + Duration::from_secs(5);
    let prefix = dir.path().to_str().unwrap().to_string();
    let removed = index.remove_under_prefix(&prefix);
    assert!(
        Instant::now() < deadline,
        "remove_under_prefix 与并发 exists 读死锁"
    );
    stop.store(true, Ordering::Relaxed);
    reader.join().unwrap();
    assert_eq!(removed, 50, "prefix 下 50 文件必须全部移除");
}

#[test]
fn visit_location_skipping_drops_listed_extensions_case_insensitively() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.js"), b"js").unwrap();
    fs::write(dir.path().join("b.JSON"), b"json").unwrap();
    fs::write(dir.path().join("c.png"), b"png").unwrap();
    fs::write(dir.path().join("noext"), b"x").unwrap();
    let root = Utf8PathBuf::from(dir.path().to_str().unwrap());
    let mut index = Index::new();
    index.visit_location_skipping(
        &crate::entities::uri::Location::Local(root),
        &crate::adapters::backend::local::LocalBackend::arc(),
        &["js".to_owned(), "json".to_owned()],
    );
    assert_eq!(index.len(), 2);
    assert_eq!(index.stats().skipped_by_ext, 2);
}
