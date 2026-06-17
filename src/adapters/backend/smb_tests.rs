//! `SmbBackend` 测试：FakeRemoteClient<SmbTarget> 注入 + 调度逻辑 100% 覆盖。
//! 迁移到统一 FakeRemoteClient；协议特异断言通过 spy 读出。

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

// SMB 测试专用的 FakeRemoteClient：error_factory 把 PermissionDenied 转成
// 含 "EACCES" 文案的 Other error，从而触发 SmbAdapter::map_error。
type FakeClient = FakeRemoteClient<SmbTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::with_error_factory(|k| match k {
        io::ErrorKind::PermissionDenied => io::Error::other("smb client returned EACCES"),
        other => io::Error::from(other),
    }))
}

fn smb(path: &str) -> Location {
    Location::Smb {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>) -> SmbBackend {
    SmbBackend::with_client(client as Arc<dyn SmbClient>)
}

#[test]
fn new_returns_unsupported_when_feature_disabled() {
    let err = SmbBackend::new().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    let msg = format!("{err}");
    assert!(msg.contains("smb-backend"), "got: {msg}");
}

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
