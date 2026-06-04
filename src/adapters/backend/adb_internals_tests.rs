//! `AdbBackend` 内部 helpers / 适配器测试：`map_error`、`AdbTarget` impl、
//! `parent_target`、`build_target`、`adb_buffered_writer`、`arc_with_client`、`shell_quote`。
//! 从 `adb_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::FakeRemoteClient;
use super::super::remote::RemoteAdapter;
use super::*;
use crate::entities::uri::Location;

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
