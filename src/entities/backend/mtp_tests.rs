//! MtpBackend 单测：FakeMtpClient 注入 + Fuzzy/Exact 匹配语义 100% 覆盖。
//! 真实 mtp-rs 适配器留作后续 PR，本测试不依赖 USB / libmtp。

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
struct FakeMtpState {
    files: HashMap<Utf8PathBuf, Vec<u8>>,
    metas: HashMap<Utf8PathBuf, Metadata>,
    op_errors: HashMap<(MtpOp, Utf8PathBuf), io::ErrorKind>,
    /// 最近一次 stat/list/read/write 调用看到的匹配参数（用于断言）
    last_target_seen: Option<MtpTarget>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum MtpOp {
    Stat,
    List,
    Read,
    Write,
    Unlink,
    Mkdir,
}

#[derive(Debug)]
struct FakeMtpClient {
    state: Arc<Mutex<FakeMtpState>>,
}

impl FakeMtpClient {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(FakeMtpState::default())),
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

    fn inject(&self, op: MtpOp, path: &str, kind: io::ErrorKind) {
        self.state
            .lock()
            .unwrap()
            .op_errors
            .insert((op, Utf8PathBuf::from(path)), kind);
    }

    fn record(&self, target: &MtpTarget) {
        self.state.lock().unwrap().last_target_seen = Some(target.clone());
    }

    fn check(&self, op: MtpOp, path: &Utf8PathBuf) -> io::Result<()> {
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.op_errors.get(&(op, path.clone())) {
            return Err(io::Error::from(*kind));
        }
        Ok(())
    }
}

impl MtpClient for FakeMtpClient {
    fn stat(&self, target: &MtpTarget) -> io::Result<Metadata> {
        self.record(target);
        self.check(MtpOp::Stat, &target.path)?;
        let s = self.state.lock().unwrap();
        s.metas
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, target: &MtpTarget) -> io::Result<Vec<Entry>> {
        self.record(target);
        self.check(MtpOp::List, &target.path)?;
        let s = self.state.lock().unwrap();
        Ok(s.metas
            .iter()
            .filter(|(p, _)| {
                let parent = target.path.as_str();
                let child = p.as_str();
                parent.is_empty()
                    || child == parent
                    || child.starts_with(&format!("{parent}/"))
            })
            .map(|(p, m)| Entry {
                location: Location::Mtp {
                    device: target.device.clone(),
                    storage: target.storage.clone(),
                    path: p.clone(),
                },
                size: m.size,
                kind: m.kind,
            })
            .collect())
    }

    fn read(&self, target: &MtpTarget) -> io::Result<Vec<u8>> {
        self.record(target);
        self.check(MtpOp::Read, &target.path)?;
        let s = self.state.lock().unwrap();
        s.files
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write(&self, target: &MtpTarget, data: &[u8]) -> io::Result<u64> {
        self.record(target);
        self.check(MtpOp::Write, &target.path)?;
        let size = data.len() as u64;
        let mut s = self.state.lock().unwrap();
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

    fn unlink(&self, target: &MtpTarget) -> io::Result<()> {
        self.record(target);
        self.check(MtpOp::Unlink, &target.path)?;
        let mut s = self.state.lock().unwrap();
        s.files.remove(&target.path);
        s.metas.remove(&target.path);
        Ok(())
    }

    fn mkdir(&self, target: &MtpTarget) -> io::Result<()> {
        self.record(target);
        self.check(MtpOp::Mkdir, &target.path)?;
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

fn mtp(path: &str) -> Location {
    Location::Mtp {
        device: "Pixel 8".into(),
        storage: "Internal shared storage".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeMtpClient>, dm: MtpMatch, sm: MtpMatch) -> MtpBackend {
    MtpBackend::with_client(client as Arc<dyn MtpClient>, dm, sm)
}

fn fuzzy_backend(client: Arc<FakeMtpClient>) -> MtpBackend {
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
    let backend = fuzzy_backend(FakeMtpClient::new());
    assert_eq!(backend.scheme(), "mtp");
}

#[test]
fn debug_format_renders_client_and_match() {
    let backend = backend_with(FakeMtpClient::new(), MtpMatch::Exact, MtpMatch::Fuzzy);
    let s = format!("{backend:?}");
    assert!(s.contains("MtpBackend"), "got: {s}");
    assert!(s.contains("device_match"));
    assert!(s.contains("Exact"));
    assert!(s.contains("Fuzzy"));
}

#[test]
fn target_records_match_mode_passed_to_client() {
    let client = FakeMtpClient::new();
    client.add_file("dir/a.bin", vec![1]);
    let backend = backend_with(client.clone(), MtpMatch::Exact, MtpMatch::Fuzzy);
    backend.metadata(&mtp("dir/a.bin")).unwrap();
    let seen = client.state.lock().unwrap().last_target_seen.clone().unwrap();
    assert_eq!(seen.device_match, MtpMatch::Exact);
    assert_eq!(seen.storage_match, MtpMatch::Fuzzy);
    assert_eq!(seen.device, "Pixel 8");
    assert_eq!(seen.storage, "Internal shared storage");
}

#[test]
fn metadata_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .metadata(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = FakeMtpClient::new();
    client.add_file("DCIM/a.jpg", vec![1, 2, 3, 4]);
    let backend = fuzzy_backend(client);
    let meta = backend.metadata(&mtp("DCIM/a.jpg")).unwrap();
    assert_eq!(meta.size, 4);
}

#[test]
fn exists_returns_true_then_false() {
    let client = FakeMtpClient::new();
    client.add_file("a.jpg", vec![1]);
    let backend = fuzzy_backend(client);
    assert!(backend.exists(&mtp("a.jpg")).unwrap());
    assert!(!backend.exists(&mtp("missing.jpg")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = FakeMtpClient::new();
    client.add_file("a.jpg", vec![1]);
    client.inject(MtpOp::Stat, "a.jpg", io::ErrorKind::PermissionDenied);
    let backend = fuzzy_backend(client);
    let err = backend.exists(&mtp("a.jpg")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = FakeMtpClient::new();
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
    let backend = fuzzy_backend(FakeMtpClient::new());
    let mut it = backend.walk(&Location::Local(Utf8PathBuf::from("/tmp/x")));
    let err = it.next().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_list_error() {
    let client = FakeMtpClient::new();
    client.inject(MtpOp::List, "DCIM", io::ErrorKind::TimedOut);
    let backend = fuzzy_backend(client);
    let mut it = backend.walk(&mtp("DCIM"));
    let err = it.next().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = FakeMtpClient::new();
    client.add_file("a.jpg", b"hello-mtp".to_vec());
    let backend = fuzzy_backend(client);
    let mut r = backend.open_read(&mtp("a.jpg")).unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello-mtp");
}

#[test]
fn open_read_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = FakeMtpClient::new();
    client.add_file("a.jpg", b"x".to_vec());
    client.inject(MtpOp::Read, "a.jpg", io::ErrorKind::Interrupted);
    let backend = fuzzy_backend(client);
    let err = backend.open_read(&mtp("a.jpg")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn open_write_buffers_and_finish_commits() {
    use std::io::Write;
    let client = FakeMtpClient::new();
    let backend = fuzzy_backend(client.clone());
    let mut w = backend.open_write(&mtp("DCIM/out.jpg"), true).unwrap();
    w.write_all(b"jpg-bytes").unwrap();
    w.finish().unwrap();
    let stored = client
        .state
        .lock()
        .unwrap()
        .files
        .get(&Utf8PathBuf::from("DCIM/out.jpg"))
        .cloned();
    assert_eq!(stored.as_deref(), Some(&b"jpg-bytes"[..]));
}

#[test]
fn open_write_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_no_mkparent_when_path_has_no_parent() {
    use std::io::Write;
    let client = FakeMtpClient::new();
    let backend = fuzzy_backend(client.clone());
    let mut w = backend.open_write(&mtp("root.jpg"), true).unwrap();
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
fn open_write_finish_propagates_client_error() {
    use std::io::Write;
    let client = FakeMtpClient::new();
    client.inject(MtpOp::Write, "x.jpg", io::ErrorKind::ConnectionAborted);
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionAborted);
}

#[test]
fn remove_file_calls_unlink() {
    let client = FakeMtpClient::new();
    client.add_file("a.jpg", vec![1]);
    let backend = fuzzy_backend(client.clone());
    backend.remove_file(&mtp("a.jpg")).unwrap();
    assert!(client.state.lock().unwrap().files.is_empty());
}

#[test]
fn remove_file_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_records_dir() {
    let client = FakeMtpClient::new();
    let backend = fuzzy_backend(client.clone());
    backend.mkdir_p(&mtp("newdir")).unwrap();
    let meta = client
        .state
        .lock()
        .unwrap()
        .metas
        .get(&Utf8PathBuf::from("newdir"))
        .cloned()
        .unwrap();
    assert_eq!(meta.kind, EntryKind::Dir);
}

#[test]
fn mkdir_p_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_decodes_utf8() {
    let client = FakeMtpClient::new();
    client.add_file("a.txt", b"hello-mtp".to_vec());
    let backend = fuzzy_backend(client);
    let s = backend.read_to_string(&mtp("a.txt")).unwrap();
    assert_eq!(s, "hello-mtp");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = FakeMtpClient::new();
    client.add_file("a.txt", vec![0xFF, 0xFE]);
    let backend = fuzzy_backend(client);
    let err = backend.read_to_string(&mtp("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = FakeMtpClient::new();
    client.add_file("a.txt", b"x".to_vec());
    client.inject(MtpOp::Read, "a.txt", io::ErrorKind::ConnectionReset);
    let backend = fuzzy_backend(client);
    let err = backend.read_to_string(&mtp("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = FakeMtpClient::new();
    client.add_file("src.jpg", b"abc".to_vec());
    let backend = fuzzy_backend(client.clone());
    let bytes = backend
        .copy_file(&mtp("src.jpg"), &mtp("DCIM/dst.jpg"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client
        .state
        .lock()
        .unwrap()
        .files
        .get(&Utf8PathBuf::from("DCIM/dst.jpg"))
        .cloned();
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_no_mkparent_when_dst_path_has_no_parent() {
    let client = FakeMtpClient::new();
    client.add_file("src.jpg", b"abc".to_vec());
    let backend = fuzzy_backend(client.clone());
    backend
        .copy_file(&mtp("src.jpg"), &mtp("dst.jpg"), true)
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
fn copy_file_rejects_non_mtp_scheme_on_either_side() {
    let backend = fuzzy_backend(FakeMtpClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.copy_file(&local, &mtp("dst"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    let err = backend.copy_file(&mtp("src"), &local, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_propagates_read_error() {
    let client = FakeMtpClient::new();
    client.add_file("src.jpg", b"x".to_vec());
    client.inject(MtpOp::Read, "src.jpg", io::ErrorKind::Interrupted);
    let backend = fuzzy_backend(client);
    let err = backend
        .copy_file(&mtp("src.jpg"), &mtp("dst.jpg"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("a/b/c.jpg"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    let p = super::parent_target(&t).unwrap();
    assert_eq!(p.path.as_str(), "a/b");
    assert_eq!(p.device, "d");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("x.jpg"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from(""),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn mtp_target_equality_and_debug() {
    let t1 = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("x"),
        device_match: MtpMatch::Exact,
        storage_match: MtpMatch::Fuzzy,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn mtp_match_distinct_variants_and_hashable() {
    use std::collections::HashSet;
    let mut s = HashSet::new();
    s.insert(MtpMatch::Exact);
    s.insert(MtpMatch::Fuzzy);
    s.insert(MtpMatch::Exact);
    assert_eq!(s.len(), 2);
    let _ = format!("{:?}", MtpMatch::Exact);
}

#[test]
fn mtp_buffered_writer_debug_format() {
    use std::io::Write;
    let client = FakeMtpClient::new();
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("MtpBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn mtp_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = FakeMtpClient::new();
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn MtpClient> = FakeMtpClient::new();
    let backend =
        MtpBackend::arc_with_client(client, MtpMatch::Fuzzy, MtpMatch::Exact);
    assert_eq!(backend.scheme(), "mtp");
}
