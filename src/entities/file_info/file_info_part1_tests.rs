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
        let (bytes_read, _fast, _full) = super::fast_hash(common::DATA_LARGE);
        assert_eq!(bytes_read, super::FAST_READ_SIZE);

        let (bytes_read, full) = super::full_hash(common::DATA_LARGE);
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
    assert!(
        got.as_str().ends_with("tests/data/data_small")
            || got.as_str().ends_with(r"tests\data\data_small"),
        "unexpected canonical path: {got}"
    );
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

// naive 时间按 UTC 解释（offset=0），既有断言基线。
fn utc0() -> chrono::FixedOffset {
    chrono::FixedOffset::east_opt(0).unwrap()
}

#[test]
fn create_time_no_exif_uses_meta() {
    let info = Info::from(common::DATA_SMALL).unwrap();
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!(secs > 0);
}

#[test]
fn create_time_uses_exif_when_valid() {
    let mut info = Info::from(common::DATA_SMALL).unwrap();
    let exif =
        super::super::exif::Exif::with_mime("image/png").with_date_time_original(1_700_000_000);
    info.set_exif(exif);
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 1_700_000_000);
}

#[test]
fn create_time_falls_back_when_exif_below_threshold() {
    let mut info = Info::from(common::DATA_SMALL).unwrap();
    let exif = super::super::exif::Exif::with_mime("image/png").with_date_time_original(100);
    info.set_exif(exif);
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
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
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 631_152_000);
}

#[test]
fn is_media_false_when_no_exif() {
    let info = Info::from(common::DATA_SMALL).unwrap();
    assert!(!info.is_media());
}

// P2 接线：无 EXIF 时文件名启发式候选必须被消费（旧实现 let-else 直接 fs 兜底）。
#[test]
fn create_time_uses_filename_candidate_without_exif() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IMG_20240501_143000.jpg");
    fs::write(&path, b"not-really-a-jpeg").unwrap();
    let info = Info::from(path.to_str().unwrap()).unwrap();
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // utc0() 下文件名 naive 时间按 UTC 解释：2024-05-01T14:30:00Z
    assert_eq!(secs, 1_714_573_800);
}

// P2 文件名 naive 时间按传入时区（非 UTC）解释：月末晚间 + 东八区若按 UTC
// 解释会 +8h 跨月归错桶（2010-08-31 23:05 当 UTC → 归 9 月）。
#[test]
fn create_time_filename_candidate_respects_tz_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("2010-08-31 23-05-36.jpg");
    fs::write(&path, b"not-really-a-jpeg").unwrap();
    let info = Info::from(path.to_str().unwrap()).unwrap();
    let east8 = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, east8);
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // 2010-08-31 23:05:36 +08:00 = 2010-08-31T15:05:36Z
    assert_eq!(secs, 1_283_267_136);
}
