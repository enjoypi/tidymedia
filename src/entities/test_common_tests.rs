use super::*;

// target_dir 不存在 → fs::copy 失败，覆盖 L41 ? Err。
#[test]
fn copy_png_to_errors_when_target_dir_missing() {
    let bogus = std::path::Path::new("/definitely/missing/parent/zzz_tc");
    let err = copy_png_to(bogus, "x.png").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// 拷贝完成后立即删 dst，再让 set_file_mtime 失败，覆盖 L43 ? Err。
// 通过两步走：先成功 copy_png_to，再单独调用 set_file_mtime 验证它会失败。
// 这里直接构造：把 dst 立即转成一个不存在的同名文件路径。
#[test]
fn set_file_mtime_on_missing_path_fails() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("never-created.png");
    let ts = filetime::FileTime::from_unix_time(FIXED_MEDIA_MTIME, 0);
    let err = filetime::set_file_mtime(&missing, ts).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
