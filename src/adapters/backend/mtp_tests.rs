//! `MtpBackend` 单测：FakeRemoteClient<MtpTarget> 注入 + Fuzzy/Exact 匹配语义 100% 覆盖。
//! 真实 mtp-rs 适配器留作后续 PR，本测试不依赖 USB / libmtp。
//! 迁移到统一 FakeRemoteClient；协议特异断言通过 spy 读出。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

// MTP 测试共用 FakeRemoteClient<MtpTarget>，默认 error_factory（不需文案注入）。
type FakeClient = FakeRemoteClient<MtpTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::new())
}

fn mtp(path: &str) -> Location {
    Location::Mtp {
        device: "Pixel 8".into(),
        storage: "Internal shared storage".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>, dm: MtpMatch, sm: MtpMatch) -> MtpBackend {
    MtpBackend::with_client(client as Arc<dyn MtpClient>, dm, sm)
}

fn fuzzy_backend(client: Arc<FakeClient>) -> MtpBackend {
    backend_with(client, MtpMatch::Fuzzy, MtpMatch::Fuzzy)
}

#[test]
fn new_returns_unsupported_when_feature_disabled() {
    let err = MtpBackend::new().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    let msg = format!("{err}");
    assert!(msg.contains("mtp-backend"), "got: {msg}");
}

#[test]
fn scheme_is_mtp() {
    let backend = fuzzy_backend(fake_client());
    assert_eq!(backend.scheme(), "mtp");
}

#[test]
fn debug_format_renders_client_and_match() {
    let backend = backend_with(fake_client(), MtpMatch::Exact, MtpMatch::Fuzzy);
    let s = format!("{backend:?}");
    assert!(s.contains("RemoteBackend"), "got: {s}");
    assert!(s.contains("mtp"));
}

#[test]
fn target_records_match_mode_passed_to_client() {
    let client = fake_client();
    client.add_file("dir/a.bin", vec![1]);
    let backend = backend_with(client.clone(), MtpMatch::Exact, MtpMatch::Fuzzy);
    backend.metadata(&mtp("dir/a.bin")).unwrap();
    let seen = client.spy.lock().unwrap().last_target_seen.clone().unwrap();
    assert_eq!(seen.device_match, MtpMatch::Exact);
    assert_eq!(seen.storage_match, MtpMatch::Fuzzy);
    assert_eq!(seen.device, "Pixel 8");
    assert_eq!(seen.storage, "Internal shared storage");
}

#[test]
fn metadata_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .metadata(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = fake_client();
    client.add_file("DCIM/a.jpg", vec![1, 2, 3, 4]);
    let backend = fuzzy_backend(client);
    let meta = backend.metadata(&mtp("DCIM/a.jpg")).unwrap();
    assert_eq!(meta.size, 4);
}

#[test]
fn exists_returns_true_then_false() {
    let client = fake_client();
    client.add_file("a.jpg", vec![1]);
    let backend = fuzzy_backend(client);
    assert!(backend.exists(&mtp("a.jpg")).unwrap());
    assert!(!backend.exists(&mtp("missing.jpg")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = fake_client();
    client.add_file("a.jpg", vec![1]);
    client.inject(RemoteFakeOp::Stat, "a.jpg", io::ErrorKind::PermissionDenied);
    let backend = fuzzy_backend(client);
    let err = backend.exists(&mtp("a.jpg")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = fake_client();
    client.add_file("DCIM/a.jpg", vec![1]);
    client.add_file("DCIM/b.jpg", vec![2]);
    client.add_file("other/c.bin", vec![3]);
    let backend = fuzzy_backend(client);
    let entries: Vec<_> = backend
        .walk(&mtp("DCIM"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn walk_propagates_target_error() {
    let backend = fuzzy_backend(fake_client());
    let mut it = backend.walk(&Location::Local(Utf8PathBuf::from("/tmp/x")));
    let err = it.next().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_list_error() {
    let client = fake_client();
    client.inject(RemoteFakeOp::List, "DCIM", io::ErrorKind::TimedOut);
    let backend = fuzzy_backend(client);
    let mut it = backend.walk(&mtp("DCIM"));
    let err = it.next().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = fake_client();
    client.add_file("a.jpg", b"hello-mtp".to_vec());
    let backend = fuzzy_backend(client);
    let mut r = backend.open_read(&mtp("a.jpg")).unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello-mtp");
}

#[test]
fn open_read_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = fake_client();
    client.add_file("a.jpg", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "a.jpg", io::ErrorKind::Interrupted);
    let backend = fuzzy_backend(client);
    let err = backend.open_read(&mtp("a.jpg")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn open_write_buffers_and_finish_commits() {
    use std::io::Write;
    let client = fake_client();
    let backend = fuzzy_backend(client.clone());
    let mut w = backend.open_write(&mtp("DCIM/out.jpg"), true).unwrap();
    w.write_all(b"jpg-bytes").unwrap();
    w.finish().unwrap();
    let stored = client.get_file("DCIM/out.jpg");
    assert_eq!(stored.as_deref(), Some(&b"jpg-bytes"[..]));
}

#[test]
fn open_write_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_no_mkparent_when_path_has_no_parent() {
    use std::io::Write;
    let client = fake_client();
    let backend = fuzzy_backend(client.clone());
    let mut w = backend.open_write(&mtp("root.jpg"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    // root.jpg 无 parent：无 mkdir，root.jpg 本身是 File
    let meta = client.get_metadata("root.jpg").unwrap();
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn open_write_finish_propagates_client_error() {
    use std::io::Write;
    let client = fake_client();
    client.inject(
        RemoteFakeOp::Write,
        "x.jpg",
        io::ErrorKind::ConnectionAborted,
    );
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionAborted);
}

#[test]
fn remove_file_calls_unlink() {
    let client = fake_client();
    client.add_file("a.jpg", vec![1]);
    let backend = fuzzy_backend(client.clone());
    backend.remove_file(&mtp("a.jpg")).unwrap();
    assert!(client.get_file("a.jpg").is_none());
}

#[test]
fn remove_file_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_records_dir() {
    let client = fake_client();
    let backend = fuzzy_backend(client.clone());
    backend.mkdir_p(&mtp("newdir")).unwrap();
    let meta = client.get_metadata("newdir").unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

#[test]
fn mkdir_p_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
