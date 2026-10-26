#[test]
fn similar_files_groups_collisions() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_SMALL_COPY).unwrap();
    let group = index.similar_paths(common::DATA_SMALL_WYHASH);
    assert_eq!(group.len(), 2);
    let small = file_info::full_path(common::DATA_SMALL).unwrap();
    let small_copy = file_info::full_path(common::DATA_SMALL_COPY).unwrap();
    assert!(group.contains(&small));
    assert!(group.contains(&small_copy));
    // 让 Utf8Path import 仍被使用
    let _ = Utf8Path::new(common::DATA_SMALL);
}

// exists(secure=true) 命中：覆盖 SHA-512 判等分支
#[test]
fn exists_secure_returns_some_for_duplicate() {
    let index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let dup = Info::from(common::DATA_SMALL_COPY).unwrap();
    let found = index
        .exists(&dup, true)
        .unwrap()
        .expect("duplicate must be detected via secure hash");
    assert_eq!(found, file_info::full_path(common::DATA_SMALL).unwrap());
}

// fast_hash 相同但 size 不同时，exists 必须 continue 不命中（覆盖 size != src.size 分支）
#[test]
fn exists_size_mismatch_skipped_even_with_fast_hash_collision() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.extend_from_slice(&[b'B'; 100]);
    fs::write(&b_path, &b).unwrap();

    let index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    let info_a = Info::from(a_path.to_str().unwrap()).unwrap();
    assert_eq!(info_a.fast_hash, info_b.fast_hash);
    assert_ne!(info_a.size, info_b.size);

    assert!(index.exists(&info_b, false).unwrap().is_none());
    assert!(index.exists(&info_b, true).unwrap().is_none());
}

// secure=true 时 index 中文件被删 → secure_hash IO Err 传播
#[test]
fn exists_secure_propagates_calc_hash_error_when_file_deleted() {
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
    let err = index.exists(&info_b, true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// secure=true 时 src 文件被删 → secure_hash IO Err 传播（右侧 ?）
#[test]
fn exists_secure_propagates_calc_hash_error_when_src_deleted() {
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
    let err = index.exists(&info_b, true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// visit_dir 必须不再尊重 .gitignore 规则（旧 ignore::Walk 默认会跳过被列入的文件）
#[test]
fn visit_dir_ignores_gitignore_rules() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.bin\n").unwrap();
    fs::write(dir.path().join("ignored.bin"), b"abcdef").unwrap();
    fs::write(dir.path().join("kept.bin"), b"012345").unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());

    let names: Vec<String> = index
        .iter()
        .filter_map(|kv| kv.key().file_name().map(std::string::ToString::to_string))
        .collect();
    assert!(
        names.iter().any(|n| n == "ignored.bin"),
        "gitignore-listed file must still be indexed; got: {names:?}"
    );
    assert!(names.iter().any(|n| n == "kept.bin"));
}

// visit_dir 累计 skipped_empty；同时安装 warn 级 subscriber 让宏内字段表达式被求值
#[test]
fn visit_dir_counts_skipped_empty_with_warn_subscriber() {
    use tracing_subscriber::EnvFilter;
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("warn"))
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("empty.bin"), b"").unwrap();
        fs::write(dir.path().join("kept.bin"), b"abcdef").unwrap();
        let mut index = Index::new();
        index.visit_dir(dir.path().to_str().unwrap());
        let s = index.stats();
        assert_eq!(s.skipped_empty, 1);
        assert_eq!(index.len(), 1);
    });
}

// visit_dir 累计 skipped_unreadable（chmod 000）
#[test]
#[cfg(unix)]
fn visit_dir_counts_skipped_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let p = dir.path().join("locked.bin");
    fs::write(&p, b"abcdef").unwrap();
    let mut perms = fs::metadata(&p).unwrap().permissions();
    let original = perms.mode();
    perms.set_mode(0o000);
    fs::set_permissions(&p, perms.clone()).unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());

    // 恢复权限以便 tempdir 清理
    perms.set_mode(original);
    fs::set_permissions(&p, perms).unwrap();

    let s = index.stats();
    assert_eq!(s.skipped_unreadable, 1);
    assert_eq!(index.len(), 0);
}

// visit_dir 对不存在 root 计 walker_errors
#[test]
fn visit_dir_counts_walker_errors_on_missing_root() {
    let mut index = Index::new();
    index.visit_dir("/no/such/dir/zzz_walker_err_xyz");
    // 精确计数：`>= 1` 杀不掉「+= 变 -=」变异（u64 release 下 wrap 成 MAX 仍 >= 1）
    assert_eq!(index.stats().walker_errors, 1);
    assert_eq!(index.len(), 0);
}

#[test]
fn visit_stats_default_is_zero() {
    let s = super::VisitStats::default();
    assert_eq!(
        s,
        super::VisitStats {
            skipped_empty: 0,
            skipped_unreadable: 0,
            walker_errors: 0,
            skipped_by_ext: 0,
        }
    );
}

#[test]
fn default_constructs_zero_state_index() {
    let index = Index::default();
    assert!(index.is_empty());
    assert_eq!(index.stats(), super::VisitStats::default());
}
