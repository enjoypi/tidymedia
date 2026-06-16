//! `AdbBackend` 测试：FakeRemoteClient<AdbTarget> 注入 + 调度逻辑 100% 覆盖。
//! 真实 `adb_client` 适配器（RealAdbClient）需 adb-server + 真机才能稳定触发，本测试不依赖 adb daemon。
//! 迁移到统一 FakeRemoteClient；协议特异断言通过 spy 读出。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

// ADB 测试共用 FakeRemoteClient<AdbTarget>，默认 error_factory（不需文案注入）。
type FakeClient = FakeRemoteClient<AdbTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::new())
}

fn adb(path: &str) -> Location {
    Location::Adb {
        serial: Some("EMULATOR5554".into()),
        path: Utf8PathBuf::from(path),
    }
}

fn adb_auto(path: &str) -> Location {
    Location::Adb {
        serial: None,
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>) -> AdbBackend {
    AdbBackend::with_client(client as Arc<dyn AdbClient>)
}

#[test]
fn new_returns_unsupported_when_feature_disabled() {
    let err = AdbBackend::new().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    let msg = format!("{err}");
    assert!(msg.contains("adb-backend"), "got: {msg}");
}

#[test]
fn scheme_is_adb() {
    let backend = backend_with(fake_client());
    assert_eq!(backend.scheme(), "adb");
}

#[test]
fn debug_format_renders_client() {
    let backend = backend_with(fake_client());
    let s = format!("{backend:?}");
    assert!(s.contains("RemoteBackend"), "got: {s}");
}

#[test]
fn metadata_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.metadata(&local).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", vec![1, 2, 3, 4]);
    let backend = backend_with(client);
    let meta = backend.metadata(&adb("/sdcard/a.bin")).unwrap();
    assert_eq!(meta.size, 4);
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn metadata_serial_autodetect_passes_through() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client);
    let meta = backend.metadata(&adb_auto("/sdcard/a.bin")).unwrap();
    assert_eq!(meta.size, 1);
}

#[test]
fn exists_returns_true_then_false() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client);
    assert!(backend.exists(&adb("/sdcard/a.bin")).unwrap());
    assert!(!backend.exists(&adb("/sdcard/missing.bin")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", vec![1]);
    client.inject(
        RemoteFakeOp::Stat,
        "/sdcard/a.bin",
        io::ErrorKind::PermissionDenied,
    );
    let backend = backend_with(client);
    let err = backend.exists(&adb("/sdcard/a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = fake_client();
    client.add_file("/sdcard/DCIM/a.bin", vec![1]);
    client.add_file("/sdcard/DCIM/b.bin", vec![2]);
    client.add_file("/sdcard/Other/c.bin", vec![3]);
    let backend = backend_with(client);
    let entries: Vec<_> = backend
        .walk(&adb("/sdcard/DCIM"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 2);
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
    client.inject(RemoteFakeOp::List, "/sdcard/DCIM", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let mut it = backend.walk(&adb("/sdcard/DCIM"));
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = fake_client();
    client.add_file("/sdcard/a.bin", b"hello".to_vec());
    let backend = backend_with(client);
    let mut r = backend.open_read(&adb("/sdcard/a.bin")).unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello");
}

#[test]
fn open_read_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "/sdcard/a.bin", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.open_read(&adb("/sdcard/a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_write_buffers_and_finish_commits() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client.clone());
    let mut w = backend
        .open_write(&adb("/sdcard/dir/out.bin"), true)
        .unwrap();
    w.write_all(b"data-bytes").unwrap();
    w.finish().unwrap();
    let bytes = client.get_file("/sdcard/dir/out.bin");
    assert_eq!(bytes.as_deref(), Some(&b"data-bytes"[..]));
}

#[test]
fn open_write_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_finish_propagates_permission_denied() {
    use std::io::Write;
    let client = fake_client();
    client.inject(
        RemoteFakeOp::Write,
        "/sdcard/x.bin",
        io::ErrorKind::PermissionDenied,
    );
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn open_write_serial_threaded_through_to_client() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    let seen = client.spy.lock().unwrap().last_target_seen.clone().unwrap();
    assert_eq!(seen.serial, Some("EMULATOR5554".into()));
}

#[test]
fn remove_file_calls_unlink() {
    let client = fake_client();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client.clone());
    backend.remove_file(&adb("/sdcard/a.bin")).unwrap();
    assert!(client.get_file("/sdcard/a.bin").is_none());
}

#[test]
fn remove_file_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_records_dir() {
    let client = fake_client();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&adb("/sdcard/newdir")).unwrap();
    let meta = client.get_metadata("/sdcard/newdir").unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

#[test]
fn mkdir_p_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
