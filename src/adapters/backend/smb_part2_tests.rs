#[test]
fn rename_default_moves_file_via_copy_remove() {
    let client = fake_client();
    client.add_file("dir/a.jpg", b"photo".to_vec());
    let backend = backend_with(client.clone());
    backend
        .rename(&smb("dir/a.jpg"), &smb("inbox/a.jpg"), false)
        .unwrap();
    assert!(client.get_file("dir/a.jpg").is_none(), "src must be gone");
    assert_eq!(
        client.get_file("inbox/a.jpg").as_deref(),
        Some(b"photo".as_ref())
    );
}

#[test]
fn rename_propagates_copy_error_and_leaves_src() {
    let client = fake_client();
    client.add_file("src.bin", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "src.bin", io::ErrorKind::TimedOut);
    let backend = backend_with(client.clone());
    let err = backend
        .rename(&smb("src.bin"), &smb("dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(client.get_file("src.bin").is_some(), "src must remain");
}

#[test]
fn rename_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.rename(&local, &smb("dst.bin"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = fake_client();
    client.add_file("a.bin", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "a.bin", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.open_read(&smb("a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

/// 触发 `remote::mkparent` 的 `if let Err(e)` 调试日志 arm：`mkparent` 调
/// `mkdir_recursive(parent)`，对 parent 的 stat 注入非 `NotFound` Err，让
/// `mkdir_recursive` 直接传播错误 → `mkparent` 进入 `debug!` 分支（被 swallow，
/// 调用方 `open_write` 仍成功，因为 client.write 是 fake 直接落盘）。
#[test]
fn open_write_mkparent_failure_swallowed_to_debug_log() {
    use std::io::Write;
    let client = fake_client();
    // 对 parent "subdir" 的 stat 注入 PermissionDenied → mkdir_recursive 直接
    // propagate Err（非 NotFound 不会触发祖先扫描），mkparent debug! 分支命中。
    client.inject(
        RemoteFakeOp::Stat,
        "subdir",
        io::ErrorKind::PermissionDenied,
    );
    let backend = backend_with(client.clone());
    // open_write mkparents=true：mkparent 内 mkdir_recursive 失败被 swallow；
    // 后续 fake client.write 直接写 path 不验父目录，整个 open_write 成功。
    let mut w = backend.open_write(&smb("subdir/dst.bin"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    let stored = client.get_file("subdir/dst.bin");
    assert_eq!(stored.as_deref(), Some(&b"x"[..]));
}

#[test]
fn open_write_no_mkparent_when_path_has_no_parent() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client.clone());
    // share 根直接文件名（path 仅一段）→ parent_target 返 None → mkparent 分支不调用 mkdir
    let mut w = backend.open_write(&smb("root.bin"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    // root.bin 无 parent，mkdir 不应被调用：root.bin 自身是 File
    let meta = client.get_metadata("root.bin").unwrap();
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn remove_file_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = fake_client();
    client.add_file("a.txt", b"hello".to_vec());
    client.inject(RemoteFakeOp::Read, "a.txt", io::ErrorKind::ConnectionReset);
    let backend = backend_with(client);
    let err = backend.read_to_string(&smb("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_no_mkparent_when_dst_path_has_no_parent() {
    let client = fake_client();
    client.add_file("src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    // dst 在 share 根：parent_target None → mkparent 分支不调用 mkdir
    backend
        .copy_file(&smb("src.bin"), &smb("dst.bin"), true)
        .unwrap();
    // dst 在 share 根：parent 无，不调 mkdir；dst 本身是 File
    let stored = client.get_file("dst.bin");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

// （`parent_target_returns_none_for_empty_path` / `arc_with_client_builds_dyn_backend`
//   / `smb_target_entry_location_constructs_uri` / `smb_target_path_returns_inner_path`
//   均已移至 smb_internals_tests.rs。）

#[test]
fn walk_skips_already_visited_directory() {
    let client = fake_client();
    client.add_dir("");
    let entries: Vec<_> = backend_with(client).walk(&smb("")).collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_ok());
}

#[derive(Debug)]
struct InvalidEntryClient;

impl RemoteClient<SmbTarget> for InvalidEntryClient {
    fn stat(&self, _: &SmbTarget) -> io::Result<Metadata> {
        unreachable!()
    }

    fn list(&self, _: &SmbTarget) -> io::Result<Vec<Entry>> {
        Ok(vec![Entry {
            location: Location::Local(Utf8PathBuf::from("/tmp/not-smb")),
            size: 0,
            kind: EntryKind::Dir,
        }])
    }

    fn read(&self, _: &SmbTarget) -> io::Result<Vec<u8>> {
        unreachable!()
    }

    fn write(&self, _: &SmbTarget, _: &[u8]) -> io::Result<u64> {
        unreachable!()
    }

    fn unlink(&self, _: &SmbTarget) -> io::Result<()> {
        unreachable!()
    }

    fn mkdir(&self, _: &SmbTarget) -> io::Result<()> {
        unreachable!()
    }
}

#[test]
fn walk_propagates_entry_target_error() {
    let backend = SmbBackend::with_client(Arc::new(InvalidEntryClient));
    let err = backend.walk(&smb("root")).next().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
