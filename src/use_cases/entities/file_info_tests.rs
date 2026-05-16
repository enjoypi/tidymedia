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
    fn small_file() -> common::Result {
        let ct = HashTest::new(common::DATA_SMALL)?;
        assert_eq!(ct.short_wyhash, common::DATA_SMALL_WYHASH);
        assert_eq!(ct.short_xxhash, common::DATA_SMALL_XXHASH);
        assert!(ct.file_size <= super::FAST_READ_SIZE);
        assert_eq!(ct.short_read, ct.file_size);
        assert_eq!(ct.short_xxhash, ct.full);
        assert_eq!(ct.secure, common::data_small_sha512());

        let f = Info::from(common::DATA_SMALL)?;
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash()?, ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash()?, common::data_small_sha512());
        assert_eq!(f.secure_hash()?, common::data_small_sha512());

        Ok(())
    }

    #[test]
    fn large_file() -> common::Result {
        let ct = HashTest::new(common::DATA_LARGE)?;
        assert_eq!(ct.short_wyhash, common::DATA_LARGE_WYHASH);
        assert_ne!(ct.short_xxhash, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.short_read, super::FAST_READ_SIZE);
        assert!(ct.short_read < ct.file_size);
        assert_eq!(ct.full, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.secure, common::data_large_sha512());

        let f = Info::from(common::DATA_LARGE)?;
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash()?, ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(f.secure_hash()?, common::data_large_sha512());

        Ok(())
    }

    #[test]
    fn bytes_read() -> common::Result {
        let meta = fs::metadata(common::DATA_LARGE)?;

        {
            let (bytes_read, _fast, _full) = super::fast_hash(common::DATA_LARGE)?;
            assert_eq!(bytes_read, super::FAST_READ_SIZE);

            let (bytes_read, full) = super::full_hash(common::DATA_LARGE)?;
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(full, common::DATA_LARGE_XXHASH);
        }

        let f = super::Info::from(common::DATA_LARGE)?;
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64);
        assert_eq!(f.calc_full_hash()?, common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());
        // no read file when twice
        assert_eq!(f.calc_full_hash()?, common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());

        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        // no read file when twice
        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        Ok(())
    }

    #[test]
    fn same_small() -> common::Result {
        let f1 = Info::from(common::DATA_SMALL)?;
        let f2 = Info::from(common::DATA_SMALL_COPY)?;

        assert_eq!(f1, f2);
        f1.calc_full_hash()?;

        assert_eq!(f1, f2);
        Ok(())
    }

    #[test]
    fn same_large() -> common::Result {
        let f1 = Info::from(common::DATA_LARGE)?;
        let f2 = Info::from(common::DATA_LARGE_COPY)?;

        assert_eq!(f1, f2);
        f1.calc_full_hash()?;

        assert_ne!(f1, f2);

        f2.calc_full_hash()?;
        assert_eq!(f1, f2);

        Ok(())
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
    fn full_path_absolute_passthrough() -> common::Result {
        let abs = if cfg!(target_os = "windows") {
            "C:\\windows\\path"
        } else {
            "/tmp"
        };
        let got = super::full_path(abs)?;
        assert_eq!(got.as_str(), abs);
        Ok(())
    }

    #[test]
    fn full_path_relative_canonicalizes() -> common::Result {
        let got = super::full_path(common::DATA_SMALL)?;
        assert!(got.is_absolute(), "expected absolute, got {got}");
        assert!(got.as_str().ends_with("tests/data/data_small")
            || got.as_str().ends_with(r"tests\data\data_small"),
            "unexpected canonical path: {got}");
        Ok(())
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
    fn info_from_empty_file_errors() -> common::Result {
        let tmp = tempfile::NamedTempFile::new()?;
        let path = tmp.path().to_str().unwrap();
        let err = Info::from(path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(err.to_string().contains("is empty"), "got: {err}");
        Ok(())
    }

    #[test]
    fn info_from_missing_path_errors() {
        let err = Info::from("definitely-missing-path-zzz999").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn create_time_no_exif_uses_meta() -> common::Result {
        let info = Info::from(common::DATA_SMALL)?;
        let t = info.create_time()?;
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(secs > 0);
        Ok(())
    }

    #[test]
    fn create_time_uses_exif_when_valid() -> common::Result {
        let mut info = Info::from(common::DATA_SMALL)?;
        let full_path = info.full_path.as_str().to_string();
        let exif: super::super::exif::Exif = serde_json::from_value(serde_json::json!({
            "SourceFile": full_path,
            "File:MIMEType": "image/png",
            "EXIF:DateTimeOriginal": 1_700_000_000_u64,
        }))?;
        info.set_exif(exif);
        let t = info.create_time()?;
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(secs, 1_700_000_000);
        Ok(())
    }

    #[test]
    fn create_time_falls_back_when_exif_below_threshold() -> common::Result {
        let mut info = Info::from(common::DATA_SMALL)?;
        let full_path = info.full_path.as_str().to_string();
        let exif: super::super::exif::Exif = serde_json::from_value(serde_json::json!({
            "SourceFile": full_path,
            "File:MIMEType": "image/png",
            "EXIF:DateTimeOriginal": 100_u64,
        }))?;
        info.set_exif(exif);
        let t = info.create_time()?;
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 配置中阈值默认 2001-01-01，回退到文件 mtime，应大于该阈值
        let threshold = super::super::super::config::config().exif.valid_date_time_secs;
        assert!(secs > threshold, "fallback should be > {threshold}; got {secs}");
        Ok(())
    }

    #[test]
    fn create_time_uses_modify_when_smaller_than_create() -> common::Result {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("ct.bin");
        fs::write(&path, b"hello")?;
        let early = filetime::FileTime::from_unix_time(631_152_000, 0);
        filetime::set_file_mtime(&path, early)?;
        let info = Info::from(path.to_str().unwrap())?;
        let t = info.create_time()?;
        let secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(secs, 631_152_000);
        Ok(())
    }

    #[test]
    fn is_media_false_when_no_exif() -> common::Result {
        let info = Info::from(common::DATA_SMALL)?;
        assert!(!info.is_media());
        Ok(())
    }

    #[test]
    fn is_media_true_when_exif_present_and_media() -> common::Result {
        let mut info = Info::from(common::DATA_SMALL)?;
        let exif: super::super::exif::Exif = serde_json::from_value(serde_json::json!({
            "SourceFile": info.full_path.as_str().to_string(),
            "File:MIMEType": "image/jpeg",
        }))?;
        info.set_exif(exif);
        assert!(info.is_media());
        Ok(())
    }

    #[test]
    fn partial_eq_differs_when_size_differs() -> common::Result {
        let small = Info::from(common::DATA_SMALL)?;
        let large = Info::from(common::DATA_LARGE)?;
        assert_ne!(small, large);
        Ok(())
    }

    #[test]
    fn info_debug_format_includes_fast_hash() -> common::Result {
        let info = Info::from(common::DATA_SMALL)?;
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("fast_hash"));
        assert!(dbg.contains("size"));
        Ok(())
    }
