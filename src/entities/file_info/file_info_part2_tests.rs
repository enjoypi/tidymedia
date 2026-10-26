// P0（EXIF DateTimeOriginal）必须压过 P2（文件名）。
#[test]
fn create_time_prefers_exif_over_filename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IMG_20240501_143000.jpg");
    fs::write(&path, b"payload").unwrap();
    let mut info = Info::from(path.to_str().unwrap()).unwrap();
    let exif =
        super::super::exif::Exif::with_mime("image/jpeg").with_date_time_original(1_700_000_000);
    info.set_exif(exif);
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 1_700_000_000);
}

// P3 注入机制：add_candidates 注入的 sidecar 候选参与裁决并压过 P4 mtime。
#[test]
fn create_time_uses_injected_sidecar_candidate() {
    use crate::entities::media_time::{Source, epoch_to_candidate};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.bin");
    fs::write(&path, b"payload").unwrap();
    let mut info = Info::from(path.to_str().unwrap()).unwrap();
    let c = epoch_to_candidate(1_600_000_000, Source::XmpSidecar, None, false)
        .expect("non-zero epoch yields candidate");
    info.add_candidates(vec![c]);
    let t = info.create_time(TEST_VALID_THRESHOLD_SECS, utc0());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 1_600_000_000);
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

// 触发 `Info::eq` line 283 `self.fast_hash == other.fast_hash` 的 false arm：
// 两个文件 size 相等但内容（fast_hash）不同。
#[test]
fn partial_eq_differs_when_fast_hash_differs_with_same_size() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    let mut data_a = vec![0u8; 1024];
    data_a[0] = b'A';
    let mut data_b = vec![0u8; 1024];
    data_b[0] = b'B';
    fs::write(&a, &data_a).unwrap();
    fs::write(&b, &data_b).unwrap();
    let info_a = Info::from(a.to_str().unwrap()).unwrap();
    let info_b = Info::from(b.to_str().unwrap()).unwrap();
    assert_eq!(info_a.size, info_b.size);
    assert_ne!(info_a, info_b);
}

#[test]
fn info_debug_format_includes_fast_hash() {
    let info = Info::from(common::DATA_SMALL).unwrap();
    let dbg = format!("{info:?}");
    assert!(dbg.contains("fast_hash"));
    assert!(dbg.contains("size"));
}

// ============= category（copy-doc/move-doc 内容类目） =============

#[test]
fn category_defaults_to_none() {
    let info = Info::from(common::DATA_SMALL).unwrap();
    assert!(info.category_ref().is_none());
}

#[test]
fn set_category_then_read_back() {
    let mut info = Info::from(common::DATA_SMALL).unwrap();
    info.set_category("invoice".to_string());
    assert_eq!(info.category_ref(), Some("invoice"));
}

// cloned_at 必须搬 category：fast-path rename 后 dst 入 output_index，丢 category
// 会让后续重跑对同文件重复分类甚至桶漂移。
#[test]
fn cloned_at_carries_category() {
    use crate::adapters::backend::local::LocalBackend;
    use crate::entities::uri::Location;

    let mut info = Info::from(common::DATA_SMALL).unwrap();
    info.set_category("contract".to_string());
    let cloned = info.cloned_at(
        Location::Local(camino::Utf8PathBuf::from("/x/y.png")),
        LocalBackend::arc(),
    );
    assert_eq!(cloned.category_ref(), Some("contract"));
}
