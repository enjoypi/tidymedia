    use std::fs;
    use std::io;
    use std::io::Read;
    use std::io::Seek;

    use sha2::Digest;
    use wyhash;
    use xxhash_rust::xxh3;

    use super::super::test_common as common;
    use super::Info;

    struct HashTest {
        short_wyhash: u64,
        short_xxhash: u64,
        short_read: usize,
        full: u64,
        file_size: usize,

        secure: super::SecureHash,
    }

    impl HashTest {
        fn new(path: &str) -> io::Result<HashTest> {
            let mut file = fs::File::open(path)?;

            let mut buffer = [0; super::FAST_READ_SIZE];
            let short_read = file.read(&mut buffer)?;
            if short_read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "File is empty",
                ));
            }

            let short_wyhash = wyhash::wyhash(&(buffer[..short_read]), 0);
            let short_xxhash = xxh3::xxh3_64(&(buffer[..short_read]));

            let mut buffer = Vec::new();
            file.seek(std::io::SeekFrom::Start(0))?;
            let file_size = file.read_to_end(&mut buffer)?;
            let full = xxh3::xxh3_64(buffer.as_slice());

            let mut hasher = sha2::Sha512::new();
            hasher.update(buffer.as_slice());
            let secure = hasher.finalize();

            Ok(HashTest {
                short_wyhash,
                short_xxhash,
                short_read,
                full,
                file_size,
                secure,
            })
        }
    }

    #[test]
    fn small_file() {
        let ct = HashTest::new(common::DATA_SMALL).unwrap();
        assert_eq!(ct.short_wyhash, common::DATA_SMALL_WYHASH);
        assert_eq!(ct.short_xxhash, common::DATA_SMALL_XXHASH);
        assert!(ct.file_size <= super::FAST_READ_SIZE);
        assert_eq!(ct.short_read, ct.file_size);
        assert_eq!(ct.short_xxhash, ct.full);
        assert_eq!(ct.secure, common::data_small_sha512());

        let f = Info::from(common::DATA_SMALL).unwrap();
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash().unwrap(), ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash().unwrap(), common::data_small_sha512());
        assert_eq!(f.secure_hash().unwrap(), common::data_small_sha512());
    }

    #[test]
    fn large_file() {
        let ct = HashTest::new(common::DATA_LARGE).unwrap();
        assert_eq!(ct.short_wyhash, common::DATA_LARGE_WYHASH);
        assert_ne!(ct.short_xxhash, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.short_read, super::FAST_READ_SIZE);
        assert!(ct.short_read < ct.file_size);
        assert_eq!(ct.full, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.secure, common::data_large_sha512());

        let f = Info::from(common::DATA_LARGE).unwrap();
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash().unwrap(), ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash().unwrap(), common::data_large_sha512());
        assert_eq!(f.secure_hash().unwrap(), common::data_large_sha512());
    }

    #[test]
    fn bytes_read() {
        let meta = fs::metadata(common::DATA_LARGE).unwrap();

        {
            let (bytes_read, _fast, _full) = super::fast_hash(common::DATA_LARGE).unwrap();
            assert_eq!(bytes_read, super::FAST_READ_SIZE);

            let (bytes_read, full) = super::full_hash(common::DATA_LARGE).unwrap();
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(full, common::DATA_LARGE_XXHASH);
        }

        let f = super::Info::from(common::DATA_LARGE).unwrap();
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64);
        assert_eq!(f.calc_full_hash().unwrap(), common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());
        // no read file when twice
        assert_eq!(f.calc_full_hash().unwrap(), common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());

        assert_eq!(f.secure_hash().unwrap(), common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        // no read file when twice
        assert_eq!(f.secure_hash().unwrap(), common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );
    }

    #[test]
    fn same_small() {
        let f1 = Info::from(common::DATA_SMALL).unwrap();
        let f2 = Info::from(common::DATA_SMALL_COPY).unwrap();

        assert_eq!(f1, f2);
        f1.calc_full_hash().unwrap();

        assert_eq!(f1, f2);
    }

    #[test]
    fn same_large() {
        let f1 = Info::from(common::DATA_LARGE).unwrap();
        let f2 = Info::from(common::DATA_LARGE_COPY).unwrap();

        assert_eq!(f1, f2);
        f1.calc_full_hash().unwrap();

        assert_ne!(f1, f2);

        f2.calc_full_hash().unwrap();
        assert_eq!(f1, f2);
    }

    #[test]
    fn strip_windows_unc_removes_prefix_only_on_windows() {
        let input = r"\\?\C:\Users\user\prj\tidymedia\tests\data\data_small";
        let got = super::strip_windows_unc(input);
        if cfg!(target_os = "windows") {
            assert_eq!(got, r"C:\Users\user\prj\tidymedia\tests\data\data_small");
        } else {
            assert_eq!(got, input);
        }
    }

    #[test]
    fn strip_windows_unc_passes_through_when_no_prefix() {
        let input = "/home/ecs-user/tidymedia/tests/data/data_small";
        assert_eq!(super::strip_windows_unc(input), input);
    }

    #[test]
    fn full_path_absolute_passthrough() {
        let abs = if cfg!(target_os = "windows") {
            "C:\\windows\\path"
        } else {
            "/tmp"
        };
        let got = super::full_path(abs).unwrap();
        assert_eq!(got.as_str(), abs);
    }

    #[test]
    fn full_path_relative_canonicalizes() {
        let got = super::full_path(common::DATA_SMALL).unwrap();
        assert!(got.is_absolute(), "expected absolute, got {got}");
        assert!(got.as_str().ends_with("tests/data/data_small")
            || got.as_str().ends_with(r"tests\data\data_small"),
            "unexpected canonical path: {got}");
    }

    #[test]
    fn full_path_missing_path_errors() {
        let err = super::full_path("definitely-not-a-real-path-xyz123").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn info_from_directory_errors() {
        let err = Info::from(common::DATA_DIR).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(err.to_string().contains("is a directory"), "got: {err}");
    }

    #[test]
    fn info_from_empty_file_errors() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let err = Info::from(path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(err.to_string().contains("is empty"), "got: {err}");
    }

    #[test]
    fn info_from_missing_path_errors() {
        let err = Info::from("definitely-missing-path-zzz999").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // 测试用阈值：2000-01-01T00:00:00Z（与配置默认值一致）。
    const TEST_VALID_THRESHOLD_SECS: u64 = 946_684_800;

    #[test]
    fn create_time_no_exif_uses_meta() {
        let info = Info::from(common::DATA_SMALL).unwrap();
        let t = info.create_time(TEST_VALID_THRESHOLD_SECS);
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(secs > 0);
    }

    #[test]
    fn create_time_uses_exif_when_valid() {
        let mut info = Info::from(common::DATA_SMALL).unwrap();
        let exif = super::super::exif::Exif::with_mime("image/png")
            .with_date_time_original(1_700_000_000);
        info.set_exif(exif);
        let t = info.create_time(TEST_VALID_THRESHOLD_SECS);
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(secs, 1_700_000_000);
    }

    #[test]
    fn create_time_falls_back_when_exif_below_threshold() {
        let mut info = Info::from(common::DATA_SMALL).unwrap();
        let exif = super::super::exif::Exif::with_mime("image/png").with_date_time_original(100);
        info.set_exif(exif);
        let t = info.create_time(TEST_VALID_THRESHOLD_SECS);
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            secs > TEST_VALID_THRESHOLD_SECS,
            "fallback should be > {TEST_VALID_THRESHOLD_SECS}; got {secs}"
        );
    }

    #[test]
    fn create_time_uses_modify_when_smaller_than_create() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ct.bin");
        fs::write(&path, b"hello").unwrap();
        let early = filetime::FileTime::from_unix_time(631_152_000, 0);
        filetime::set_file_mtime(&path, early).unwrap();
        let info = Info::from(path.to_str().unwrap()).unwrap();
        let t = info.create_time(TEST_VALID_THRESHOLD_SECS);
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(secs, 631_152_000);
    }

    #[test]
    fn is_media_false_when_no_exif() {
        let info = Info::from(common::DATA_SMALL).unwrap();
        assert!(!info.is_media());
    }

    #[test]
    fn is_media_true_when_exif_present_and_media() {
        let mut info = Info::from(common::DATA_SMALL).unwrap();
        info.set_exif(super::super::exif::Exif::with_mime("image/jpeg"));
        assert!(info.is_media());
    }

    #[test]
    fn partial_eq_differs_when_size_differs() {
        let small = Info::from(common::DATA_SMALL).unwrap();
        let large = Info::from(common::DATA_LARGE).unwrap();
        assert_ne!(small, large);
    }

    #[test]
    fn info_debug_format_includes_fast_hash() {
        let info = Info::from(common::DATA_SMALL).unwrap();
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("fast_hash"));
        assert!(dbg.contains("size"));
    }

    // 绝对路径直接跳过 canonicalize（full_path 内 is_absolute() 分支），随后 metadata() 失败。
    // 触发 file_info.rs L71 metadata()? 的 Err region。
    #[test]
    fn info_from_absolute_missing_path_errors() {
        let err = Info::from("/definitely/missing/zzz_abs_path_xyz").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // 文件 metadata 可读但 read 不可（chmod 000），让 fast_hash 内 File::open 失败。
    // 触发 file_info.rs L86 + L206/L209 的 Err region。
    // 注意：测试结束前需恢复权限，否则 tempdir 清理会失败。
    #[test]
    #[cfg(unix)]
    fn info_from_unreadable_file_errors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.bin");
        fs::write(&path, b"non-empty content").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&path, perms.clone()).unwrap();

        let err = Info::from(path.to_str().unwrap()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        // 恢复权限，让 tempdir 在测试结束清理时能删除该文件
        perms.set_mode(0o644);
        fs::set_permissions(&path, perms).unwrap();
    }

    // Info 实例创建后立刻删除底层文件，再调 calc_full_hash → mmap 打开失败。
    // 触发 file_info.rs L112 + L218/L219 的 Err region。
    #[test]
    fn calc_full_hash_errors_when_file_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vanishing.bin");
        fs::write(&path, b"contents that will vanish").unwrap();
        let info = Info::from(path.to_str().unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        let err = info.calc_full_hash().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // 同上，但走 secure_hash 路径。触发 L130 + L225/L226 的 Err region。
    #[test]
    fn secure_hash_errors_when_file_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vanishing2.bin");
        fs::write(&path, b"contents that will vanish 2").unwrap();
        let info = Info::from(path.to_str().unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        let err = info.secure_hash().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    use std::time::Duration;
    use std::time::SystemTime;

    /// pick_fs_fallback：modified < created（罕见但合法）→ 取 modified。
    #[test]
    fn pick_fs_fallback_modified_smaller_than_created() {
        let m = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let c = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        let got = super::pick_fs_fallback(Some(m), Some(c));
        assert_eq!(got.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 100);
    }

    /// pick_fs_fallback：modified ≥ created → 取 created。
    #[test]
    fn pick_fs_fallback_modified_ge_created() {
        let m = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        let c = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let got = super::pick_fs_fallback(Some(m), Some(c));
        assert_eq!(got.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 100);
    }

    /// pick_fs_fallback：created 不可用（btime 缺失），只看 modified。
    #[test]
    fn pick_fs_fallback_created_none() {
        let m = SystemTime::UNIX_EPOCH + Duration::from_secs(50);
        let got = super::pick_fs_fallback(Some(m), None);
        assert_eq!(got.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 50);
    }

    /// pick_fs_fallback：modified 不可用（极端 fs），只看 created。
    #[test]
    fn pick_fs_fallback_modified_none() {
        let c = SystemTime::UNIX_EPOCH + Duration::from_secs(75);
        let got = super::pick_fs_fallback(None, Some(c));
        assert_eq!(got.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 75);
    }

    /// pick_fs_fallback：两个时间都不可用 → UNIX_EPOCH 兜底。
    #[test]
    fn pick_fs_fallback_both_none() {
        let got = super::pick_fs_fallback(None, None);
        assert_eq!(got, SystemTime::UNIX_EPOCH);
    }
