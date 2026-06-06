//! `AdbBackend` 文件操作测试：`read_to_string` / `copy_file` / rename / root-path 边界（从 `adb_tests.rs` 拆出）。

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

fn backend_with(client: Arc<FakeClient>) -> AdbBackend {
    AdbBackend::with_client(client as Arc<dyn AdbClient>)
}
#[test]
fn read_to_string_decodes_utf8() {
    let client = fake_client();
    client.add_file("/sdcard/a.txt", b"hello \xe4\xb8\xad".to_vec());
    let backend = backend_with(client);
    let s = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap();
    assert_eq!(s, "hello \u{4e2d}");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = fake_client();
    client.add_file("/sdcard/a.txt", vec![0xFF, 0xFE]);
    let backend = backend_with(client);
    let err = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = fake_client();
    client.add_file("/sdcard/a.txt", b"hello".to_vec());
    client.inject(
        RemoteFakeOp::Read,
        "/sdcard/a.txt",
        io::ErrorKind::ConnectionReset,
    );
    let backend = backend_with(client);
    let err = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = fake_client();
    client.add_file("/sdcard/src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    let bytes = backend
        .copy_file(&adb("/sdcard/src.bin"), &adb("/sdcard/sub/dst.bin"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client.get_file("/sdcard/sub/dst.bin");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_rejects_non_adb_scheme_on_either_side() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend
        .copy_file(&local, &adb("/sdcard/dst"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    let err = backend
        .copy_file(&adb("/sdcard/src"), &local, false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_propagates_read_error() {
    let client = fake_client();
    client.add_file("/sdcard/src.bin", b"x".to_vec());
    client.inject(
        RemoteFakeOp::Read,
        "/sdcard/src.bin",
        io::ErrorKind::Interrupted,
    );
    let backend = backend_with(client);
    let err = backend
        .copy_file(&adb("/sdcard/src.bin"), &adb("/sdcard/dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn copy_file_no_mkparent_when_dst_at_root() {
    let client = fake_client();
    client.add_file("/src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    // dst 在设备根 `/`：parent_target None → mkparent 无，dst 是 File
    backend
        .copy_file(&adb("/src.bin"), &adb("/dst.bin"), true)
        .unwrap();
    let stored = client.get_file("/dst.bin");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn open_write_no_mkparent_when_path_at_root() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&adb("/root.bin"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    // /root.bin 的 parent 是 /，被 AdbTarget::parent 过滤 → 无 mkdir
    let meta = client.get_metadata("/root.bin").unwrap();
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn rename_default_moves_file_via_copy_remove() {
    let client = fake_client();
    client.add_file("/sdcard/a.jpg", b"photo".to_vec());
    let backend = backend_with(client.clone());
    backend
        .rename(&adb("/sdcard/a.jpg"), &adb("/sdcard/inbox/a.jpg"), false)
        .unwrap();
    assert!(
        client.get_file("/sdcard/a.jpg").is_none(),
        "src must be gone"
    );
    assert_eq!(
        client.get_file("/sdcard/inbox/a.jpg").as_deref(),
        Some(b"photo".as_ref())
    );
}

#[test]
fn rename_propagates_copy_error_and_leaves_src() {
    let client = fake_client();
    client.add_file("/sdcard/src.bin", b"x".to_vec());
    client.inject(
        RemoteFakeOp::Read,
        "/sdcard/src.bin",
        io::ErrorKind::TimedOut,
    );
    let backend = backend_with(client.clone());
    let err = backend
        .rename(&adb("/sdcard/src.bin"), &adb("/sdcard/dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(
        client.get_file("/sdcard/src.bin").is_some(),
        "src must remain"
    );
}

#[test]
fn rename_rejects_non_adb_scheme() {
    let backend = backend_with(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend
        .rename(&local, &adb("/sdcard/dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
