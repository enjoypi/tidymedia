//! SmbBackend 测试：FakeSmbClient 注入 + 调度逻辑 100% 覆盖。
//! 真实 smb crate 适配器（RealSmbClient）留作未来 PR，本测试不依赖任何 SMB server。

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
struct FakeSmbState {
    files: HashMap<Utf8PathBuf, Vec<u8>>,
    metas: HashMap<Utf8PathBuf, Metadata>,
    /// 注入"调用此 op + 此 path"时 client 返回的错误
    op_errors: HashMap<(SmbOp, Utf8PathBuf), io::ErrorKind>,
    /// `write` 调用时记录看到的 target.password（用于断言 env 凭据传递）
    last_password_seen: Option<Option<String>>,
    last_krb5_seen: Option<Option<String>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum SmbOp {
    Stat,
    List,
    Read,
    Write,
    Unlink,
    Mkdir,
}

#[derive(Debug)]
struct FakeSmbClient {
    state: Arc<Mutex<FakeSmbState>>,
}

impl FakeSmbClient {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(FakeSmbState::default())),
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

    fn inject(&self, op: SmbOp, path: &str, kind: io::ErrorKind) {
        let mut s = self.state.lock().unwrap();
        s.op_errors.insert((op, Utf8PathBuf::from(path)), kind);
    }

    fn check(&self, op: SmbOp, path: &Utf8PathBuf) -> io::Result<()> {
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.op_errors.get(&(op, path.clone())) {
            // 用 Other + "EACCES" 文案触发 map_smb_error 的特殊映射
            if *kind == io::ErrorKind::PermissionDenied {
                return Err(io::Error::other("smb client returned EACCES"));
            }
            return Err(io::Error::from(*kind));
        }
        Ok(())
    }
}

impl SmbClient for FakeSmbClient {
    fn stat(&self, target: &SmbTarget) -> io::Result<Metadata> {
        self.check(SmbOp::Stat, &target.path)?;
        let s = self.state.lock().unwrap();
        s.metas
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, target: &SmbTarget) -> io::Result<Vec<Entry>> {
        self.check(SmbOp::List, &target.path)?;
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
                location: Location::Smb {
                    user: target.user.clone(),
                    host: target.host.clone(),
                    port: target.port,
                    share: target.share.clone(),
                    path: p.clone(),
                },
                size: m.size,
                kind: m.kind,
            })
            .collect())
    }

    fn read(&self, target: &SmbTarget) -> io::Result<Vec<u8>> {
        self.check(SmbOp::Read, &target.path)?;
        let s = self.state.lock().unwrap();
        s.files
            .get(&target.path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write(&self, target: &SmbTarget, data: &[u8]) -> io::Result<u64> {
        self.check(SmbOp::Write, &target.path)?;
        let size = data.len() as u64;
        let mut s = self.state.lock().unwrap();
        s.last_password_seen = Some(target.password.clone());
        s.last_krb5_seen = Some(target.krb5_ccname.clone());
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

    fn unlink(&self, target: &SmbTarget) -> io::Result<()> {
        self.check(SmbOp::Unlink, &target.path)?;
        let mut s = self.state.lock().unwrap();
        s.files.remove(&target.path);
        s.metas.remove(&target.path);
        Ok(())
    }

    fn mkdir(&self, target: &SmbTarget) -> io::Result<()> {
        self.check(SmbOp::Mkdir, &target.path)?;
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

fn smb(path: &str) -> Location {
    Location::Smb {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeSmbClient>) -> SmbBackend {
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
    let backend = backend_with(FakeSmbClient::new());
    assert_eq!(backend.scheme(), "smb");
}

#[test]
fn debug_format_renders_client() {
    let backend = backend_with(FakeSmbClient::new());
    let s = format!("{backend:?}");
    assert!(s.contains("SmbBackend"), "got: {s}");
}

#[test]
fn metadata_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.metadata(&local).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn metadata_returns_size_for_known_file() {
    let client = FakeSmbClient::new();
    client.add_file("dir/a.bin", vec![1, 2, 3, 4]);
    let backend = backend_with(client);
    let meta = backend.metadata(&smb("dir/a.bin")).unwrap();
    assert_eq!(meta.size, 4);
    assert_eq!(meta.kind, EntryKind::File);
}

#[test]
fn exists_returns_true_then_false() {
    let client = FakeSmbClient::new();
    client.add_file("a.bin", vec![1]);
    let backend = backend_with(client);
    assert!(backend.exists(&smb("a.bin")).unwrap());
    assert!(!backend.exists(&smb("missing.bin")).unwrap());
}

#[test]
fn exists_propagates_non_notfound_error() {
    let client = FakeSmbClient::new();
    client.add_file("a.bin", vec![1]);
    client.inject(SmbOp::Stat, "a.bin", io::ErrorKind::PermissionDenied);
    let backend = backend_with(client);
    let err = backend.exists(&smb("a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_lists_files_under_root() {
    let client = FakeSmbClient::new();
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
fn walk_propagates_target_error() {
    let backend = backend_with(FakeSmbClient::new());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let mut it = backend.walk(&local);
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_list_error() {
    let client = FakeSmbClient::new();
    client.inject(SmbOp::List, "dir", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let mut it = backend.walk(&smb("dir"));
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_returns_buffered_reader() {
    use std::io::Read;
    let client = FakeSmbClient::new();
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
    let client = FakeSmbClient::new();
    let backend = backend_with(client.clone());
    let mut w = backend.open_write(&smb("dir/out.bin"), true).unwrap();
    w.write_all(b"data-bytes").unwrap();
    w.finish().unwrap();
    // 由 FakeSmbClient::write 写回
    let bytes = client.state.lock().unwrap().files.get(&Utf8PathBuf::from("dir/out.bin")).cloned();
    assert_eq!(bytes.as_deref(), Some(&b"data-bytes"[..]));
}

#[test]
fn open_write_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let err = backend
        .open_write(&Location::Local(Utf8PathBuf::from("/tmp")), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_finish_propagates_eacces_to_permission_denied() {
    use std::io::Write;
    let client = FakeSmbClient::new();
    client.inject(SmbOp::Write, "x.bin", io::ErrorKind::PermissionDenied);
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    w.write_all(b"data").unwrap();
    let err = w.finish().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn remove_file_calls_unlink() {
    let client = FakeSmbClient::new();
    client.add_file("a.bin", vec![1]);
    let backend = backend_with(client.clone());
    backend.remove_file(&smb("a.bin")).unwrap();
    assert!(client.state.lock().unwrap().files.is_empty());
}

#[test]
fn mkdir_p_records_dir() {
    let client = FakeSmbClient::new();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&smb("newdir")).unwrap();
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
fn read_to_string_decodes_utf8() {
    let client = FakeSmbClient::new();
    client.add_file("a.txt", b"hello \xe4\xb8\xad".to_vec());
    let backend = backend_with(client);
    let s = backend.read_to_string(&smb("a.txt")).unwrap();
    assert_eq!(s, "hello \u{4e2d}");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = FakeSmbClient::new();
    client.add_file("a.txt", vec![0xFF, 0xFE]);
    let backend = backend_with(client);
    let err = backend.read_to_string(&smb("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = FakeSmbClient::new();
    client.add_file("src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    let bytes = backend
        .copy_file(&smb("src.bin"), &smb("subdir/dst.bin"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client
        .state
        .lock()
        .unwrap()
        .files
        .get(&Utf8PathBuf::from("subdir/dst.bin"))
        .cloned();
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_rejects_non_smb_scheme_on_either_side() {
    let backend = backend_with(FakeSmbClient::new());
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
    let client = FakeSmbClient::new();
    client.add_file("src.bin", b"x".to_vec());
    client.inject(SmbOp::Read, "src.bin", io::ErrorKind::Interrupted);
    let backend = backend_with(client);
    let err = backend
        .copy_file(&smb("src.bin"), &smb("dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn map_smb_error_eacces_to_permission_denied() {
    let e = io::Error::other("smb client returned EACCES");
    let mapped = super::map_smb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_smb_error_passthrough_other_kinds() {
    let e = io::Error::from(io::ErrorKind::TimedOut);
    let mapped = super::map_smb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn map_smb_error_passthrough_other_without_eacces() {
    let e = io::Error::other("disk full");
    let mapped = super::map_smb_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
    assert!(format!("{mapped}").contains("disk full"));
}

#[test]
fn smb_buffered_writer_debug_format() {
    use std::io::Write;
    let client = FakeSmbClient::new();
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("SmbBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn smb_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = FakeSmbClient::new();
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn parent_target_returns_none_for_root_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("only.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b/c.bin"),
        password: None,
        krb5_ccname: None,
    };
    let p = super::parent_target(&t).unwrap();
    assert_eq!(p.path.as_str(), "a/b");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    // Utf8PathBuf::from("x.bin").parent() == Some("")，要走 if-empty 早返回
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn build_target_threads_env_password_and_krb5() {
    // 用 nextest 进程隔离让 set_var 安全；其他测试不会被污染
    unsafe {
        std::env::set_var("SMB_PASSWORD", "secret-pw");
        std::env::set_var("KRB5CCNAME", "/tmp/krb5cc_0");
    }
    let t = super::build_target(&smb("a.bin")).unwrap();
    assert_eq!(t.password.as_deref(), Some("secret-pw"));
    assert_eq!(t.krb5_ccname.as_deref(), Some("/tmp/krb5cc_0"));
    assert_eq!(t.user.as_deref(), Some("alice"));
    assert_eq!(t.host, "nas");
    assert_eq!(t.port, Some(445));
    assert_eq!(t.share, "photos");
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
}

#[test]
fn build_target_leaves_password_none_when_env_unset() {
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
    let t = super::build_target(&smb("a.bin")).unwrap();
    assert!(t.password.is_none());
    assert!(t.krb5_ccname.is_none());
}

#[test]
fn smb_target_equality_and_debug() {
    let t1 = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x"),
        password: None,
        krb5_ccname: None,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn open_read_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let err = backend
        .open_read(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_propagates_client_error() {
    let client = FakeSmbClient::new();
    client.add_file("a.bin", b"x".to_vec());
    client.inject(SmbOp::Read, "a.bin", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.open_read(&smb("a.bin")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_write_no_mkparent_when_path_has_no_parent() {
    use std::io::Write;
    let client = FakeSmbClient::new();
    let backend = backend_with(client.clone());
    // share 根直接文件名（path 仅一段）→ parent_target 返 None → mkparent 分支不调用 mkdir
    let mut w = backend.open_write(&smb("root.bin"), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    // mkdir 未被调用：metas 中没有任何 Dir entry
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
fn remove_file_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let err = backend
        .remove_file(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_rejects_non_smb_scheme() {
    let backend = backend_with(FakeSmbClient::new());
    let err = backend
        .mkdir_p(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = FakeSmbClient::new();
    client.add_file("a.txt", b"hello".to_vec());
    client.inject(SmbOp::Read, "a.txt", io::ErrorKind::ConnectionReset);
    let backend = backend_with(client);
    let err = backend.read_to_string(&smb("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_no_mkparent_when_dst_path_has_no_parent() {
    let client = FakeSmbClient::new();
    client.add_file("src.bin", b"abc".to_vec());
    let backend = backend_with(client.clone());
    // dst 在 share 根：parent_target None → mkparent 分支不调用 mkdir
    backend
        .copy_file(&smb("src.bin"), &smb("dst.bin"), true)
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
fn parent_target_returns_none_for_empty_path() {
    // 空字符串 path 让 Utf8Path::parent() 直接返 None，命中 `?` early return
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from(""),
        password: None,
        krb5_ccname: None,
    };
    assert!(super::parent_target(&t).is_none());
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn SmbClient> = FakeSmbClient::new();
    let backend = SmbBackend::arc_with_client(client);
    assert_eq!(backend.scheme(), "smb");
}
