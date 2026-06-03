//! `AdbBackend` 测试：FakeRemoteClient<AdbTarget> 注入 + 调度逻辑 100% 覆盖。
//! 真实 `adb_client` 适配器（RealAdbClient）需 adb-server + 真机才能稳定触发，
//! 整模块 coverage(off)；本测试不依赖 adb daemon。
//! 迁移到统一 FakeRemoteClient；协议特异断言通过 spy 读出。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use super::super::remote::RemoteAdapter;
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
fn map_adb_error_not_found_via_no_such_file() {
    let e = io::Error::other("adb: no such file or directory");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_not_found_via_does_not_exist() {
    let e = io::Error::other("remote path does not exist");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_permission_denied() {
    let e = io::Error::other("adb: permission denied");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_adb_error_device_not_found_to_notfound() {
    let e = io::Error::other("device not found");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_no_devices_to_notfound() {
    let e = io::Error::other("error: no devices/emulators found");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_passthrough_other_without_known_pattern() {
    let e = io::Error::other("some unknown adb failure");
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
    assert!(format!("{mapped}").contains("unknown adb failure"));
}

#[test]
fn map_adb_error_passthrough_non_other_kinds() {
    let e = io::Error::from(io::ErrorKind::TimedOut);
    let mapped = AdbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/a/b/c.bin"),
    };
    let p = t.parent().unwrap();
    assert_eq!(p.path.as_str(), "/a/b");
}

#[test]
fn parent_target_returns_none_for_root_child() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/only.bin"),
    };
    // parent("/only.bin") == Some("/")，被 if-root 早返回
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from(""),
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_none_when_parent_is_empty_string() {
    // 单 component 相对路径：`Utf8Path::parent("file.txt") == Some("")`，
    // 触发 L50 `parent.as_str().is_empty()` 的 True 分支（区别于 "" 直接走
    // `?` 早返回，进不到 if-block）。
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("file.txt"),
    };
    assert!(t.parent().is_none());
}

#[test]
fn build_target_threads_serial_and_path() {
    let t = AdbTarget::from_location(&adb("/sdcard/a.bin"), &()).unwrap();
    assert_eq!(t.serial.as_deref(), Some("EMULATOR5554"));
    assert_eq!(t.path.as_str(), "/sdcard/a.bin");
}

#[test]
fn build_target_autodetect_serial_none() {
    let t = AdbTarget::from_location(&adb_auto("/sdcard/a.bin"), &()).unwrap();
    assert!(t.serial.is_none());
    assert_eq!(t.path.as_str(), "/sdcard/a.bin");
}

#[test]
fn adb_target_equality_and_debug() {
    let t1 = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/x"),
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn adb_buffered_writer_debug_format() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn adb_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn AdbClient> = fake_client();
    let backend = AdbBackend::arc_with_client(client);
    assert_eq!(backend.scheme(), "adb");
}

#[test]
fn shell_quote_wraps_in_single_quotes() {
    assert_eq!(super::shell_quote("simple"), "'simple'");
}

#[test]
fn shell_quote_escapes_inner_single_quote() {
    // 单引号 → '\'' 续接序列
    assert_eq!(super::shell_quote("a'b"), "'a'\\''b'");
}

#[test]
fn shell_quote_preserves_spaces_and_paths() {
    assert_eq!(
        super::shell_quote("/sdcard/My Photos/foto.jpg"),
        "'/sdcard/My Photos/foto.jpg'"
    );
}

#[test]
fn shell_quote_empty_string_renders_empty_pair() {
    assert_eq!(super::shell_quote(""), "''");
}

#[test]
fn adb_target_entry_location_constructs_uri() {
    let t = AdbTarget {
        serial: Some("EMULATOR5554".into()),
        path: Utf8PathBuf::from("/sdcard/a.jpg"),
    };
    let loc = t.entry_location(Utf8PathBuf::from("/sdcard/b.jpg"));
    assert_eq!(
        loc,
        Location::Adb {
            serial: Some("EMULATOR5554".into()),
            path: Utf8PathBuf::from("/sdcard/b.jpg"),
        }
    );
}

#[test]
fn adb_target_path_returns_inner_path() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/sdcard/DCIM/photo.jpg"),
    };
    assert_eq!(t.path().as_str(), "/sdcard/DCIM/photo.jpg");
}
