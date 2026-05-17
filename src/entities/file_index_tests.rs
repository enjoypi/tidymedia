    use std::collections::BTreeMap;
    use std::fmt;
    use std::fs;

    use camino::Utf8Path;
    use tempfile::tempdir;

    use super::super::file_info;
    use super::super::test_common as common;
    use super::Index;
    use super::Info;

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

        let same: BTreeMap<u64, _> = index.search_same();
        assert_eq!(same.len(), 2);
        assert_eq!(same[&common::DATA_LARGE_LEN].len(), 2);
        assert_eq!(same[&common::DATA_SMALL_LEN].len(), 2);

        let large_path = file_info::full_path(common::DATA_LARGE).unwrap();
        let large_copy = file_info::full_path(common::DATA_LARGE_COPY).unwrap();
        let small_path = file_info::full_path(common::DATA_SMALL).unwrap();
        let small_copy = file_info::full_path(common::DATA_SMALL_COPY).unwrap();
        assert!(same[&common::DATA_LARGE_LEN].contains(&large_path));
        assert!(same[&common::DATA_LARGE_LEN].contains(&large_copy));
        assert!(same[&common::DATA_SMALL_LEN].contains(&small_path));
        assert!(same[&common::DATA_SMALL_LEN].contains(&small_copy));
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
        let found = index.exists(&copy, false).unwrap().expect("duplicate must be detected");
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
        let mut b = prefix.clone();
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
        index.add(first).unwrap();
        let again = Info::from(common::DATA_SMALL).unwrap();
        index.add(again).unwrap();
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
        let total: u64 = index.files().values().map(|f| f.bytes_read()).sum();
        assert_eq!(index.bytes_read(), total);
    }

    #[test]
    fn parse_exif_empty_index_ok() {
        let mut index = Index::new();
        index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap());
        assert_eq!(index.files().len(), 0);
    }

    /// 文件在 visit_dir 之后被删除 → Exif::from_path 返回 Err →
    /// parse_exif 内 `if let Ok` 的 Err 分支被覆盖，对应 entry 保留无 exif。
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
        let secure: BTreeMap<u64, _> = index.search_same();
        let fast: BTreeMap<u64, _> = index.fast_search_same();
        assert_eq!(secure, fast);
    }

    #[test]
    fn index_debug_format_renders_files() {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL).unwrap();
        let dbg = format!("{:?}", index);
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
        let res = fmt::write(&mut FailingWriter, format_args!("{:?}", index));
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
        let mut b = prefix.clone();
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
        let mut b = prefix.clone();
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
        let mut b = prefix.clone();
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

    #[test]
    fn similar_files_groups_collisions() {
        let mut index = Index::new();
        index.insert(common::DATA_SMALL).unwrap();
        index.insert(common::DATA_SMALL_COPY).unwrap();
        let group = index
            .similar_files()
            .get(&common::DATA_SMALL_WYHASH)
            .expect("collision group present");
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
        let mut index = Index::new();
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
        let mut b = prefix.clone();
        b.extend_from_slice(&[b'B'; 100]);
        fs::write(&b_path, &b).unwrap();

        let mut index = Index::new();
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
        let mut b = prefix.clone();
        b.push(b'B');
        fs::write(&b_path, &b).unwrap();

        let mut index = Index::new();
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
        let mut b = prefix.clone();
        b.push(b'B');
        fs::write(&b_path, &b).unwrap();

        let mut index = Index::new();
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
            .files()
            .keys()
            .filter_map(|p| p.file_name().map(|s| s.to_string()))
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
            assert_eq!(index.files().len(), 1);
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
        assert_eq!(index.files().len(), 0);
    }

    // visit_dir 对不存在 root 计 walker_errors
    #[test]
    fn visit_dir_counts_walker_errors_on_missing_root() {
        let mut index = Index::new();
        index.visit_dir("/no/such/dir/zzz_walker_err_xyz");
        assert!(index.stats().walker_errors >= 1);
        assert_eq!(index.files().len(), 0);
    }

    #[test]
    fn visit_stats_default_is_zero() {
        let s = super::VisitStats::default();
        assert_eq!(s, super::VisitStats { skipped_empty: 0, skipped_unreadable: 0, walker_errors: 0 });
    }

    #[test]
    fn default_constructs_zero_state_index() {
        let index: Index = Default::default();
        assert!(index.files().is_empty());
        assert_eq!(index.stats(), super::VisitStats::default());
    }

    // 同一 Index 承载两个不同 backend 的 visit_location 调用：
    // - FakeBackend(smb)：放 1 个 1KiB 文件
    // - FakeBackend(mtp)：放 1 个不同字节序列的 1KiB 文件
    // 期望：files() 含两条记录，fast_hash 不同；Info 内部 backend 句柄各自归属。
    #[test]
    fn visit_location_accepts_multiple_backends_in_one_index() {
        use std::sync::Arc;

        use camino::Utf8PathBuf;

        use crate::entities::backend::fake::FakeBackend;
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
        let mtp_root = Location::Mtp {
            device: "Pixel".into(),
            storage: "Internal".into(),
            path: Utf8PathBuf::new(),
        };
        let mtp_file = Location::Mtp {
            device: "Pixel".into(),
            storage: "Internal".into(),
            path: Utf8PathBuf::from("b.jpg"),
        };

        let smb = Arc::new(FakeBackend::new("smb"));
        smb.add_dir(smb_root.clone());
        smb.add_file(smb_file.clone(), vec![0xAA; 1024]);

        let mtp = Arc::new(FakeBackend::new("mtp"));
        mtp.add_dir(mtp_root.clone());
        mtp.add_file(mtp_file.clone(), vec![0x55; 1024]);

        let mut index = Index::new();
        index.visit_location(&smb_root, Arc::clone(&smb) as Arc<dyn Backend>);
        index.visit_location(&mtp_root, Arc::clone(&mtp) as Arc<dyn Backend>);

        let files = index.files();
        assert_eq!(files.len(), 2, "both backends contributed one file each");

        let smb_key = Utf8PathBuf::from(smb_file.display());
        let mtp_key = Utf8PathBuf::from(mtp_file.display());
        assert!(files.contains_key(&smb_key));
        assert!(files.contains_key(&mtp_key));
        assert_ne!(
            files[&smb_key].fast_hash,
            files[&mtp_key].fast_hash,
            "distinct byte content should hash differently"
        );

        // 重新算 full_hash 必须走各自 Info 内部的 Arc<dyn Backend>——
        // 若实现退化为单 backend 共享，跨 scheme 的 open_read 会失败。
        assert!(files[&smb_key].calc_full_hash().is_ok());
        assert!(files[&mtp_key].calc_full_hash().is_ok());
    }

    // 文件名含非 UTF-8 字节时，Utf8PathBuf::from_path_buf 失败 → 计 walker_errors
    #[test]
    #[cfg(unix)]
    fn visit_dir_counts_non_utf8_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempdir().unwrap();
        let bad = OsStr::from_bytes(&[b'a', 0xFF, 0xFE, b'.', b'b', b'i', b'n']);
        let p = dir.path().join(bad);
        fs::write(&p, b"abc").unwrap();

        let mut index = Index::new();
        index.visit_dir(dir.path().to_str().unwrap());
        assert!(index.stats().walker_errors >= 1, "non-UTF-8 path must bump walker_errors");
        assert_eq!(index.files().len(), 0);
    }

