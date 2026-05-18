//! AdbBackend 测试：FakeAdbClient 注入 + 调度逻辑 100% 覆盖。
//! 真实 adb_client 适配器（RealAdbClient）需 adb-server + 真机才能稳定触发，
//! 整模块 coverage(off)；本测试不依赖 adb daemon。

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
struct FakeAdbState {
    files: HashMap<Utf8PathBuf, Vec<u8>>,
    metas: HashMap<Utf8PathBuf, Metadata>,
    /// 注入"调用此 op + 此 path"时 client 返回的错误
    op_errors: HashMap<(AdbOp, Utf8PathBuf), io::ErrorKind>,
    /// `write` 调用时记录看到的 target.serial（用于断言 serial 透传）
    last_serial_seen: Option<Option<String>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum AdbOp {
    Stat,
    List,
    Read,
    Write,
    Unlink,
    Mkdir,
}

#[derive(Debug)]
struct FakeAdbClient {
    state: Arc<Mutex<FakeAdbState>>,
}

impl FakeAdbClient {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(FakeAdbState::default())),
        })
    }

    fn add_file(&self, path: &str, data: Vec<u8>) {
        let p = Utf8PathBuf::from(path);
        let size = data.len() as u64;
        let mut s = self.state.lock().unwrap();
        s.files.insert(p.clone(), data);
        s.metas.insert(
            p,
            Metadata {
                size,
                kind: EntryKind::File,
                modified: Some(SystemTime::UNIX_EPOCH),
                created: Some(SystemTime::UNIX_EPOCH),
            },
        );
    }

    fn inject(&self, op: AdbOp, path: &str, kind: io::ErrorKind) {
        let mut s = self.state.lock().unwrap();
        s.op_errors.insert((op, Utf8PathBuf::from(path)), kind);
    }

    fn check(&self, op: AdbOp, path: &Utf8PathBuf) -> io::Result<()> {
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.op_errors.get(&(op, path.clone())) {
            // 用 Other + 特征文案触发 map_adb_error 的特殊映射
            match *kind {
                io::ErrorKind::NotFound => {
                    return Err(io::Error::other("adb: no such file or directory"));
                }
                io::ErrorKind::PermissionDenied => {
                    return Err(io::Error::other("adb: permission denied"));
                }
                _ => return Err(io::Error::from(*kind)),
            }
        }
        Ok(())
    }
}

impl AdbClient for FakeAdbClient {
    fn stat(&self, target: &AdbTarget) -> io::Result<Metadata> {
        self.check(AdbOp::Stat, &target.path)?;
        let s = self.state.lock().unwrap();
        s.metas
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, target: &AdbTarget) -> io::Result<Vec<Entry>> {
        self.check(AdbOp::List, &target.path)?;
        let s = self.state.lock().unwrap();
        Ok(s.metas
            .iter()
            .filter(|(p, _)| {
                let parent = target.path.as_str();
                let child = p.as_str();
                child == parent || child.starts_with(&format!("{parent}/"))
            })
            .map(|(p, m)| Entry {
                location: Location::Adb {
                    serial: target.serial.clone(),
                    path: p.clone(),
                },
                size: m.size,
                kind: m.kind,
            })
            .collect())
    }

    fn read(&self, target: &AdbTarget) -> io::Result<Vec<u8>> {
        self.check(AdbOp::Read, &target.path)?;
        let s = self.state.lock().unwrap();
        s.files
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write(&self, target: &AdbTarget, data: &[u8]) -> io::Result<u64> {
        self.check(AdbOp::Write, &target.path)?;
        let size = data.len() as u64;
        let mut s = self.state.lock().unwrap();
        s.last_serial_seen = Some(target.serial.clone());
        s.files.insert(target.path.clone(), data.to_vec());
        s.metas.insert(
            target.path.clone(),
            Metadata {
                size,
                kind: EntryKind::File,
                modified: Some(SystemTime::UNIX_EPOCH),
                created: Some(SystemTime::UNIX_EPOCH),
            },
        );
        Ok(size)
    }

    fn unlink(&self, target: &AdbTarget) -> io::Result<()> {
        self.check(AdbOp::Unlink, &target.path)?;
        let mut s = self.state.lock().unwrap();
        s.files.remove(&target.path);
        s.metas.remove(&target.path);
        Ok(())
    }

    fn mkdir(&self, target: &AdbTarget) -> io::Result<()> {
        self.check(AdbOp::Mkdir, &target.path)?;
        let mut s = self.state.lock().unwrap();
        s.metas.insert(
            target.path.clone(),
            Metadata {
                size: 0,
                kind: EntryKind::Dir,
                modified: None,
                created: None,
            },
        );
        Ok(())
    }
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

fn backend_with(client: Arc<FakeAdbClient>) -> AdbBackend {
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
    let backend = backend_with(FakeAdbClient::new());
    assert_eq!(backend.scheme(), "adb");
}

#[test]
fn debug_format_renders_client() {
    let backend = backend_with(FakeAdbClient::new());
    let s = format!("{backend:?}");
    assert!(s.contains("AdbBackend"), "got: {s}");
}

#[test]
fn metadata_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.metadata(&local).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", vec![1, 2, 3, 4]);
    let backend = backend_with(client);
    let meta = backend.metadata(&adb("/sdcard/a.bin")).unwrap();
    assert_eq!(meta.size, 4);
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn metadata_serial_autodetect_passes_through() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client);
    let meta = backend.metadata(&adb_auto("/sdcard/a.bin")).unwrap();
    assert_eq!(meta.size, 1);
}

#[test]
fn exists_returns_true_then_false() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client);
    assert!(backend.exists(&adb("/sdcard/a.bin")).unwrap());
    assert!(!backend.exists(&adb("/sdcard/missing.bin")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", vec![1]);
    client.inject(AdbOp::Stat, "/sdcard/a.bin", io::ErrorKind::PermissionDenied);
    let backend = backend_with(client);
    let err = backend.exists(&adb("/sdcard/a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = FakeAdbClient::new();
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
    let backend = backend_with(FakeAdbClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let mut it = backend.walk(&local);
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_list_error() {
    let client = FakeAdbClient::new();
    client.inject(AdbOp::List, "/sdcard/DCIM", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let mut it = backend.walk(&adb("/sdcard/DCIM"));
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", b"hello".to_vec());
    let backend = backend_with(client);
    let mut r = backend.open_read(&adb("/sdcard/a.bin")).unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello");
}

#[test]
fn open_read_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", b"x".to_vec());
    client.inject(AdbOp::Read, "/sdcard/a.bin", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.open_read(&adb("/sdcard/a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_write_buffers_and_finish_commits() {
    use std::io::Write;
    let client = FakeAdbClient::new();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&adb("/sdcard/dir/out.bin"), true).unwrap();
    w.write_all(b"data-bytes").unwrap();
    w.finish().unwrap();
    let bytes = client
        .state
        .lock()
        .unwrap()
        .files
        .get(&Utf8PathBuf::from("/sdcard/dir/out.bin"))
        .cloned();
    assert_eq!(bytes.as_deref(), Some(&b"data-bytes"[..]));
}

#[test]
fn open_write_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_finish_propagates_permission_denied() {
    use std::io::Write;
    let client = FakeAdbClient::new();
    client.inject(AdbOp::Write, "/sdcard/x.bin", io::ErrorKind::PermissionDenied);
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn open_write_serial_threaded_through_to_client() {
    use std::io::Write;
    let client = FakeAdbClient::new();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    let seen = client.state.lock().unwrap().last_serial_seen.clone();
    assert_eq!(seen, Some(Some("EMULATOR5554".into())));
}

#[test]
fn remove_file_calls_unlink() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.bin", vec![1]);
    let backend = backend_with(client.clone());
    backend.remove_file(&adb("/sdcard/a.bin")).unwrap();
    assert!(client.state.lock().unwrap().files.is_empty());
}

#[test]
fn remove_file_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_records_dir() {
    let client = FakeAdbClient::new();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&adb("/sdcard/newdir")).unwrap();
    let meta = client
        .state
        .lock()
        .unwrap()
        .metas
        .get(&Utf8PathBuf::from("/sdcard/newdir"))
        .cloned()
        .unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

#[test]
fn mkdir_p_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_decodes_utf8() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.txt", b"hello \xe4\xb8\xad".to_vec());
    let backend = backend_with(client);
    let s = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap();
    assert_eq!(s, "hello \u{4e2d}");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.txt", vec![0xFF, 0xFE]);
    let backend = backend_with(client);
    let err = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_adb_scheme() {
    let backend = backend_with(FakeAdbClient::new());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/a.txt", b"hello".to_vec());
    client.inject(AdbOp::Read, "/sdcard/a.txt", io::ErrorKind::ConnectionReset);
    let backend = backend_with(client);
    let err = backend.read_to_string(&adb("/sdcard/a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    let bytes = backend
        .copy_file(&adb("/sdcard/src.bin"), &adb("/sdcard/sub/dst.bin"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client
        .state
        .lock()
        .unwrap()
        .files
        .get(&Utf8PathBuf::from("/sdcard/sub/dst.bin"))
        .cloned();
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_rejects_non_adb_scheme_on_either_side() {
    let backend = backend_with(FakeAdbClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.copy_file(&local, &adb("/sdcard/dst"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    let err = backend.copy_file(&adb("/sdcard/src"), &local, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_propagates_read_error() {
    let client = FakeAdbClient::new();
    client.add_file("/sdcard/src.bin", b"x".to_vec());
    client.inject(AdbOp::Read, "/sdcard/src.bin", io::ErrorKind::Interrupted);
    let backend = backend_with(client);
    let err = backend
        .copy_file(&adb("/sdcard/src.bin"), &adb("/sdcard/dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn copy_file_no_mkparent_when_dst_at_root() {
    let client = FakeAdbClient::new();
    client.add_file("/src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    // dst 在设备根 `/`：parent_target None → mkparent 分支不调用 mkdir
    backend
        .copy_file(&adb("/src.bin"), &adb("/dst.bin"), true)
        .unwrap();
    let dir_count = client
        .state
        .lock()
        .unwrap()
        .metas
        .values()
        .filter(|m| m.kind == EntryKind::Dir)
        .count();
    assert_eq!(dir_count, 0);
}

#[test]
fn open_write_no_mkparent_when_path_at_root() {
    use std::io::Write;
    let client = FakeAdbClient::new();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&adb("/root.bin"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    let dir_count = client
        .state
        .lock()
        .unwrap()
        .metas
        .values()
        .filter(|m| m.kind == EntryKind::Dir)
        .count();
    assert_eq!(dir_count, 0);
}

#[test]
fn map_adb_error_not_found_via_no_such_file() {
    let e = io::Error::other("adb: no such file or directory");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_not_found_via_does_not_exist() {
    let e = io::Error::other("remote path does not exist");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_permission_denied() {
    let e = io::Error::other("adb: permission denied");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_adb_error_device_not_found_to_notfound() {
    let e = io::Error::other("device not found");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_no_devices_to_notfound() {
    let e = io::Error::other("error: no devices/emulators found");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_adb_error_passthrough_other_without_known_pattern() {
    let e = io::Error::other("some unknown adb failure");
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
    assert!(format!("{mapped}").contains("unknown adb failure"));
}

#[test]
fn map_adb_error_passthrough_non_other_kinds() {
    let e = io::Error::from(io::ErrorKind::TimedOut);
    let mapped = super::map_adb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/a/b/c.bin"),
    };
    let p = super::parent_target(&t).unwrap();
    assert_eq!(p.path.as_str(), "/a/b");
}

#[test]
fn parent_target_returns_none_for_root_child() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from("/only.bin"),
    };
    // parent("/only.bin") == Some("/")，被 if-root 早返回
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    let t = AdbTarget {
        serial: None,
        path: Utf8PathBuf::from(""),
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn build_target_threads_serial_and_path() {
    let t = super::build_target(&adb("/sdcard/a.bin")).unwrap();
    assert_eq!(t.serial.as_deref(), Some("EMULATOR5554"));
    assert_eq!(t.path.as_str(), "/sdcard/a.bin");
}

#[test]
fn build_target_autodetect_serial_none() {
    let t = super::build_target(&adb_auto("/sdcard/a.bin")).unwrap();
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
    let client = FakeAdbClient::new();
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("AdbBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn adb_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = FakeAdbClient::new();
    let backend = backend_with(client);
    let mut w = backend.open_write(&adb("/sdcard/x.bin"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn AdbClient> = FakeAdbClient::new();
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
