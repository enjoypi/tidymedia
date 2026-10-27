#[test]
fn scheme_is_smb() {
    let backend = backend_with(fake_client());
    assert_eq!(backend.scheme(), "smb");
}

#[test]
fn debug_format_renders_client() {
    let backend = backend_with(fake_client());
    let s = format!("{backend:?}");
    assert!(s.contains("RemoteBackend"), "got: {s}");
}

#[test]
fn metadata_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.metadata(&local).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = fake_client();
    client.add_file("dir/a.bin", vec![1, 2, 3, 4]);
    let backend = backend_with(client);
    let meta = backend.metadata(&smb("dir/a.bin")).unwrap();
    assert_eq!(meta.size, 4);
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn exists_returns_true_then_false() {
    let client = fake_client();
    client.add_file("a.bin", vec![1]);
    let backend = backend_with(client);
    assert!(backend.exists(&smb("a.bin")).unwrap());
    assert!(!backend.exists(&smb("missing.bin")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = fake_client();
    client.add_file("a.bin", vec![1]);
    client.inject(RemoteFakeOp::Stat, "a.bin", io::ErrorKind::PermissionDenied);
    let backend = backend_with(client);
    let err = backend.exists(&smb("a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = fake_client();
    client.add_file("dir/a.bin", vec![1]);
    client.add_file("dir/b.bin", vec![2]);
    client.add_file("other/c.bin", vec![3]);
    let backend = backend_with(client);
    let entries: Vec<_> = backend
        .walk(&smb("dir"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn walk_recurses_into_subdirectory() {
    // 多层 fixture：/root 含 a.bin + /root/sub 子目录 + /root/sub/c.bin。
    // 验证 walk_recursive 的 EntryKind::Dir 分支能驱动子目录下钻并 yield 嵌套 file。
    let client = fake_client();
    client.add_file("root/a.bin", vec![1]);
    client.add_dir("root/sub");
    client.add_file("root/sub/c.bin", vec![2]);
    let backend = backend_with(client);
    let entries: Vec<_> = backend
        .walk(&smb("root"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    // 期望：a.bin (file) + sub (dir) + sub/c.bin (file) = 3 entries（dir 也 yield）。
    assert_eq!(entries.len(), 3, "got: {entries:?}");
    let nested = entries
        .iter()
        .find(|e| e.location.display().ends_with("c.bin"))
        .expect("nested c.bin must appear");
    assert_eq!(nested.kind, crate::entities::backend::EntryKind::File);
}

#[test]
fn walk_propagates_target_error() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let mut it = backend.walk(&local);
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_list_error() {
    let client = fake_client();
    client.inject(RemoteFakeOp::List, "dir", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let mut it = backend.walk(&smb("dir"));
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = fake_client();
    client.add_file("a.bin", b"hello".to_vec());
    let backend = backend_with(client);
    let mut r = backend.open_read(&smb("a.bin")).unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello");
}

#[test]
fn open_write_buffers_and_finish_commits() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&smb("dir/out.bin"), true).unwrap();
    w.write_all(b"data-bytes").unwrap();
    w.finish().unwrap();
    let bytes = client.get_file("dir/out.bin");
    assert_eq!(bytes.as_deref(), Some(&b"data-bytes"[..]));
}

#[test]
fn open_write_rejects_existing_target_without_overwrite() {
    let client = fake_client();
    client.add_file("x.bin", b"old".to_vec());
    let backend = backend_with(client.clone());
    let err = backend.open_write(&smb("x.bin"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(client.get_file("x.bin").as_deref(), Some(&b"old"[..]));
}

#[test]
fn open_write_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_finish_propagates_eacces_to_permission_denied() {
    use std::io::Write;
    let client = fake_client();
    client.inject(
        RemoteFakeOp::Write,
        "x.bin",
        io::ErrorKind::PermissionDenied,
    );
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn remove_file_calls_unlink() {
    let client = fake_client();
    client.add_file("a.bin", vec![1]);
    let backend = backend_with(client.clone());
    backend.remove_file(&smb("a.bin")).unwrap();
    assert!(client.get_file("a.bin").is_none());
}

#[test]
fn mkdir_p_records_dir() {
    let client = fake_client();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&smb("newdir")).unwrap();
    let meta = client.get_metadata("newdir").unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

// 祖先已存在时自底向上的 stat 在该层命中（mkdir_recursive 的 Ok => break），
// 只补缺失的叶层，不重复 mkdir 既有目录。
#[test]
fn mkdir_p_stops_at_existing_ancestor() {
    let client = fake_client();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&smb("existing")).unwrap();
    backend.mkdir_p(&smb("existing/sub")).unwrap();
    let meta = client.get_metadata("existing/sub").unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

#[test]
fn read_to_string_decodes_utf8() {
    let client = fake_client();
    client.add_file("a.txt", b"hello \xe4\xb8\xad".to_vec());
    let backend = backend_with(client);
    let s = backend.read_to_string(&smb("a.txt")).unwrap();
    assert_eq!(s, "hello \u{4e2d}");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = fake_client();
    client.add_file("a.txt", vec![0xFF, 0xFE]);
    let backend = backend_with(client);
    let err = backend.read_to_string(&smb("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_smb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = fake_client();
    client.add_file("src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    let bytes = backend
        .copy_file(&smb("src.bin"), &smb("subdir/dst.bin"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client.get_file("subdir/dst.bin");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_rejects_non_smb_scheme_on_either_side() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    // src 非 smb
    let err = backend.copy_file(&local, &smb("dst"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    // dst 非 smb
    let err = backend.copy_file(&smb("src"), &local, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_propagates_read_error() {
    let client = fake_client();
    client.add_file("src.bin", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "src.bin", io::ErrorKind::Interrupted);
    let backend = backend_with(client);
    let err = backend
        .copy_file(&smb("src.bin"), &smb("dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}
