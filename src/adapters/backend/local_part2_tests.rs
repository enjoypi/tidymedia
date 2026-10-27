#[test]
fn rename_or_fallback_uses_copy_and_remove_on_crosses_devices() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    fs::write(&src, b"hello").unwrap();
    super::rename_or_fallback_with(
        &src,
        &dst,
        rename_cross_devices_mock,
        super::real_copy,
        super::real_remove,
    )
    .unwrap();
    assert!(dst.is_file());
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
}

#[test]
fn rename_or_fallback_propagates_non_crosses_devices_err() {
    fn rename_perm_denied(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
    let err = super::rename_or_fallback_with(
        std::path::Path::new("/x"),
        std::path::Path::new("/y"),
        rename_perm_denied,
        super::real_copy,
        super::real_remove,
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

// CrossesDevices fallback 中 copy 失败传播：触发 `copy(from, to)?` 的 `?` Err arm。
fn copy_fails_mock(_: &std::path::Path, _: &std::path::Path) -> io::Result<u64> {
    Err(io::Error::other("inject copy fail"))
}

#[test]
fn rename_or_fallback_propagates_copy_err_in_fallback() {
    let err = super::rename_or_fallback_with(
        std::path::Path::new("/x"),
        std::path::Path::new("/y"),
        rename_cross_devices_mock,
        copy_fails_mock,
        super::real_remove,
    )
    .unwrap_err();
    assert!(err.to_string().contains("inject copy fail"), "{err}");
}

// walk_entry_to_io 中 entry.metadata() Err 触发 "metadata failed" 文案。
// 真实 ignore::DirEntry.metadata() 仅在并发删除等罕见情况失败，用 fn pointer 注入 mock。
fn meta_fail_mock(_entry: &ignore::DirEntry) -> Result<std::fs::Metadata, ignore::Error> {
    Err(ignore::Error::Io(io::Error::other("inject meta fail")))
}

#[test]
fn walk_entry_to_io_propagates_metadata_failure() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.bin"), b"x").unwrap();
    let entry = ignore::WalkBuilder::new(dir.path())
        .build()
        .filter_map(Result::ok)
        .find(|e| e.file_type().is_some_and(|t| t.is_file()))
        .expect("real DirEntry");
    let err = super::walk_entry_to_io_with(Ok(entry), meta_fail_mock).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("metadata failed for"), "got: {msg}");
    assert!(
        msg.contains("inject meta fail"),
        "must wrap inner err: {msg}"
    );
}

// CrossesDevices fallback 中 copy 成功但 remove 失败：触发 `remove(from).map_err(|re| ...)`
// 闭包，验证 "copied … but cannot remove source" 半态文案。
fn remove_fails_mock(_: &std::path::Path) -> io::Result<()> {
    Err(io::Error::other("inject remove fail"))
}

#[test]
fn rename_or_fallback_marks_half_state_when_remove_fails_in_fallback() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    fs::write(&src, b"hello").unwrap();
    let err = super::rename_or_fallback_with(
        &src,
        &dst,
        rename_cross_devices_mock,
        super::real_copy,
        remove_fails_mock,
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("copied") && msg.contains("but cannot remove source"),
        "half-state must be labelled, got: {msg}"
    );
    assert!(
        msg.contains("inject remove fail"),
        "must wrap original error: {msg}"
    );
}

// `ignore_to_io` 处理 ignore::Error 的 io_error()==None 分支（如 Loop variant，
// symlink loop 不易在 CI 稳定触发；直接构造 enum variant 测 helper）。
#[test]
fn ignore_to_io_handles_loop_variant_with_no_io_error() {
    let loop_err = ignore::Error::Loop {
        ancestor: std::path::PathBuf::from("/a"),
        child: std::path::PathBuf::from("/a/b"),
    };
    let io_err = super::ignore_to_io(&loop_err);
    // io_error() 为 None → 走 io::Error::other 分支，kind 是 Other
    assert_eq!(io_err.kind(), io::ErrorKind::Other);
}

#[test]
fn ignore_to_io_handles_io_variant_propagates_kind() {
    let inner = io::Error::from(io::ErrorKind::PermissionDenied);
    let ig = ignore::Error::Io(inner);
    let io_err = super::ignore_to_io(&ig);
    assert_eq!(io_err.kind(), io::ErrorKind::PermissionDenied);
}

fn mock_open_fails(_: &std::path::Path) -> io::Result<fs::File> {
    Err(io::Error::other("inject open fail"))
}

#[test]
fn open_read_inner_propagates_open_err() {
    let err = super::open_read_inner_with(std::path::Path::new("/x/nope.bin"), mock_open_fails)
        .unwrap_err();
    assert!(err.to_string().contains("inject open fail"), "{err}");
}

#[test]
fn open_read_inner_reads_and_seeks_buffered() {
    let dir = tempdir().unwrap();
    let f = dir.path().join("ok.bin");
    fs::write(&f, b"hello").unwrap();
    let mut r = super::open_read_inner_with(&f, super::local_io::real_open).unwrap();
    let mut buf = String::new();
    r.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "hello");
    r.seek(SeekFrom::Start(1)).unwrap();
    let mut rest = String::new();
    r.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "ello");
}
