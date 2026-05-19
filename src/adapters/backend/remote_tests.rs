//! remote.rs 测试：用 DummyTarget + 可配置 DummyClient 覆盖
//! RemoteBackend / RemoteBufferedWriter 的全部分支。

use super::*;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;
use camino::{Utf8Path, Utf8PathBuf};
use std::io;
use std::sync::Arc;

// ── DummyTarget ──────────────────────────────────────────────

/// Ctx 携带可选的 from_location 注入错误和路径覆写。
#[derive(Clone, Debug)]
struct DummyCtx {
    from_loc_err: Option<io::ErrorKind>,
    /// 覆写 from_location 返回的路径；None 时默认 "/dummy"
    path_override: Option<Utf8PathBuf>,
}

impl DummyCtx {
    fn ok() -> Self {
        Self {
            from_loc_err: None,
            path_override: None,
        }
    }
    fn with_err(kind: io::ErrorKind) -> Self {
        Self {
            from_loc_err: Some(kind),
            path_override: None,
        }
    }
    fn with_root_path() -> Self {
        Self {
            from_loc_err: None,
            path_override: Some(Utf8PathBuf::from("/")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyTarget {
    path: Utf8PathBuf,
    /// 为测试 parent() == None 设根路径
    is_root: bool,
}

impl DummyTarget {
    fn new(path: &str) -> Self {
        Self {
            path: Utf8PathBuf::from(path),
            is_root: false,
        }
    }
    fn root() -> Self {
        Self {
            path: Utf8PathBuf::from("/"),
            is_root: true,
        }
    }
}

impl RemoteTarget for DummyTarget {
    type Ctx = DummyCtx;

    fn from_location(_loc: &Location, ctx: &DummyCtx) -> io::Result<Self> {
        if let Some(kind) = ctx.from_loc_err {
            return Err(io::Error::new(
                kind,
                format!("injected from_location error: {kind:?}"),
            ));
        }
        let path = ctx
            .path_override
            .clone()
            .unwrap_or_else(|| Utf8PathBuf::from("/dummy"));
        let is_root = path.as_str() == "/";
        Ok(DummyTarget { path, is_root })
    }

    fn parent(&self) -> Option<Self> {
        if self.is_root {
            return None;
        }
        let parent = self.path.parent()?;
        if parent.as_str().is_empty() {
            return None;
        }
        Some(DummyTarget {
            path: parent.to_path_buf(),
            is_root: parent.as_str() == "/",
        })
    }

    fn entry_location(&self, p: Utf8PathBuf) -> Location {
        Location::Local(p.into())
    }

    fn path(&self) -> &Utf8Path {
        &self.path
    }
}

// ── DummyClient（按方法注入 ErrorKind）────────────────────────

#[derive(Debug, Default)]
struct DummyClient {
    stat_err: Option<io::ErrorKind>,
    list_err: Option<io::ErrorKind>,
    read_err: Option<io::ErrorKind>,
    write_err: Option<io::ErrorKind>,
    unlink_err: Option<io::ErrorKind>,
    mkdir_err: Option<io::ErrorKind>,
}

impl DummyClient {
    fn with_stat_err(k: io::ErrorKind) -> Self {
        Self {
            stat_err: Some(k),
            ..Default::default()
        }
    }
    fn with_list_err(k: io::ErrorKind) -> Self {
        Self {
            list_err: Some(k),
            ..Default::default()
        }
    }
    fn with_read_err(k: io::ErrorKind) -> Self {
        Self {
            read_err: Some(k),
            ..Default::default()
        }
    }
}

impl RemoteClient<DummyTarget> for DummyClient {
    fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
        if let Some(k) = self.stat_err {
            return Err(io::Error::from(k));
        }
        Ok(Metadata {
            size: 42,
            kind: EntryKind::File,
            modified: None,
            created: None,
        })
    }
    fn list(&self, _t: &DummyTarget) -> io::Result<Vec<Entry>> {
        if let Some(k) = self.list_err {
            return Err(io::Error::from(k));
        }
        Ok(vec![])
    }
    fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
        if let Some(k) = self.read_err {
            return Err(io::Error::from(k));
        }
        Ok(b"hello".to_vec())
    }
    fn write(&self, _t: &DummyTarget, data: &[u8]) -> io::Result<u64> {
        if let Some(k) = self.write_err {
            return Err(io::Error::from(k));
        }
        Ok(data.len() as u64)
    }
    fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
        if let Some(k) = self.unlink_err {
            return Err(io::Error::from(k));
        }
        Ok(())
    }
    fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
        if let Some(k) = self.mkdir_err {
            return Err(io::Error::from(k));
        }
        Ok(())
    }
}

// ── DummyAdapter ─────────────────────────────────────────────

struct DummyAdapter {
    client: Arc<dyn RemoteClient<DummyTarget>>,
    ctx: DummyCtx,
}

impl DummyAdapter {
    fn with_client(client: Arc<dyn RemoteClient<DummyTarget>>) -> Self {
        Self {
            client,
            ctx: DummyCtx::ok(),
        }
    }
    fn with_client_and_ctx(
        client: Arc<dyn RemoteClient<DummyTarget>>,
        ctx: DummyCtx,
    ) -> Self {
        Self { client, ctx }
    }
}

impl RemoteAdapter for DummyAdapter {
    type Target = DummyTarget;
    fn scheme() -> &'static str {
        "dummy"
    }
    fn ctx(&self) -> &DummyCtx {
        &self.ctx
    }
    fn client(&self) -> &Arc<dyn RemoteClient<DummyTarget>> {
        &self.client
    }
}

fn backend() -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend { adapter: DummyAdapter::with_client(client) }
}

fn loc() -> Location {
    Location::Local("/dummy".into())
}

fn backend_with_client(c: DummyClient) -> RemoteBackend<DummyAdapter> {
    RemoteBackend { adapter: DummyAdapter::with_client(Arc::new(c)) }
}

fn backend_with_from_loc_err(kind: io::ErrorKind) -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend {
        adapter: DummyAdapter::with_client_and_ctx(
            client,
            DummyCtx::with_err(kind),
        ),
    }
}

fn backend_with_client_and_root(
    c: DummyClient,
) -> RemoteBackend<DummyAdapter> {
    RemoteBackend { adapter: DummyAdapter::with_client(Arc::new(c)) }
}

// ── 成功路径 ──────────────────────────────────────────────────

#[test]
fn scheme_returns_dummy() {
    assert_eq!(backend().scheme(), "dummy");
}

#[test]
fn debug_format_includes_scheme() {
    let s = format!("{:?}", backend());
    assert!(s.contains("dummy"));
}

#[test]
fn metadata_ok() {
    let m = backend().metadata(&loc()).unwrap();
    assert_eq!(m.size, 42);
}

#[test]
fn exists_true_when_stat_ok() {
    assert!(backend().exists(&loc()).unwrap());
}

#[test]
fn exists_false_when_stat_not_found() {
    let b = backend_with_client(DummyClient::with_stat_err(io::ErrorKind::NotFound));
    assert!(!b.exists(&loc()).unwrap());
}

#[test]
fn exists_propagates_other_stat_error() {
    let b = backend_with_client(DummyClient::with_stat_err(io::ErrorKind::PermissionDenied));
    let e = b.exists(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_ok() {
    let entries: Vec<_> = backend().walk(&loc()).collect();
    assert_eq!(entries.len(), 0);
}

#[test]
fn walk_list_err_propagates() {
    let b = backend_with_client(DummyClient::with_list_err(io::ErrorKind::TimedOut));
    let mut iter = b.walk(&loc());
    let e = iter.next().unwrap().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_ok() {
    let mut r = backend().open_read(&loc()).unwrap();
    let mut buf = Vec::new();
    io::Read::read_to_end(&mut r, &mut buf).unwrap();
    assert_eq!(buf, b"hello");
}

#[test]
fn open_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::ConnectionRefused));
    let e = b.open_read(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
}

#[test]
fn open_write_mkparents_false() {
    let w = backend().open_write(&loc(), false).unwrap();
    let s = format!("{:?}", w);
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn open_write_mkparents_true_ok() {
    let w = backend().open_write(&loc(), true).unwrap();
    let s = format!("{:?}", w);
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn remove_file_ok() {
    backend().remove_file(&loc()).unwrap();
}

#[test]
fn remove_file_err_propagates() {
    let mut c = DummyClient::default();
    c.unlink_err = Some(io::ErrorKind::NotFound);
    let b = backend_with_client(c);
    let e = b.remove_file(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn mkdir_p_ok() {
    backend().mkdir_p(&loc()).unwrap();
}

#[test]
fn read_to_string_ok() {
    let s = backend().read_to_string(&loc()).unwrap();
    assert_eq!(s, "hello");
}

#[test]
fn read_to_string_invalid_utf8() {
    let mut c = DummyClient::default();
    // 返回非 UTF-8 字节
    c.read_err = None; // allow read to succeed, but override to return bad bytes
    // 需要另一个 client 变体来返回非 UTF-8
    // 用元组注入方式：这里用一个专门返回乱码的 client
    #[derive(Debug)]
    struct BadUtf8Client;
    impl RemoteClient<DummyTarget> for BadUtf8Client {
        fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
            unreachable!()
        }
        fn list(&self, _t: &DummyTarget) -> io::Result<Vec<Entry>> {
            unreachable!()
        }
        fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
            Ok(vec![0xff, 0xfe]) // invalid UTF-8
        }
        fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
            unreachable!()
        }
        fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
        fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
    }
    let b = RemoteBackend { adapter: DummyAdapter::with_client(Arc::new(BadUtf8Client)) };
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::TimedOut));
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn copy_file_ok_no_mkparents() {
    let n = backend().copy_file(&loc(), &loc(), false).unwrap();
    assert_eq!(n, 5);
}

#[test]
fn copy_file_ok_with_mkparents() {
    let n = backend().copy_file(&loc(), &loc(), true).unwrap();
    assert_eq!(n, 5);
}

#[test]
fn copy_file_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::ConnectionReset));
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionReset);
}

// ── RemoteBufferedWriter ──────────────────────────────────────

#[test]
fn buffered_writer_write_and_flush() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let mut w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: Vec::new(),
    };
    let n = io::Write::write(&mut w, b"abc").unwrap();
    assert_eq!(n, 3);
    io::Write::flush(&mut w).unwrap();
}

#[test]
fn buffered_writer_debug_shows_buffered_bytes() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: vec![0u8; 10],
    };
    let s = format!("{:?}", w);
    assert!(s.contains("buffered_bytes"));
    assert!(s.contains("10"));
}

#[test]
fn buffered_writer_finish_writes_through() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: b"data".to_vec(),
    };
    Box::new(w).finish().unwrap();
}

#[test]
fn buffered_writer_finish_write_err_propagates() {
    let mut c = DummyClient::default();
    c.write_err = Some(io::ErrorKind::TimedOut);
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(c);
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: b"data".to_vec(),
    };
    let e = Box::new(w).finish().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

// ── build_target Err（from_location 注入错误） ────────────────

#[test]
fn metadata_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.metadata(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let mut iter = b.walk(&loc());
    let e = iter.next().unwrap().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.open_read(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.open_write(&loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn remove_file_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.remove_file(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.mkdir_p(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_src_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

// ── parent() == None 分支 ─────────────────────────────────────

fn backend_with_root_ctx() -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend {
        adapter: DummyAdapter::with_client_and_ctx(
            client,
            DummyCtx::with_root_path(),
        ),
    }
}

#[test]
fn open_write_mkparents_with_root_skips_mkdir() {
    // from_location 返回根 target → parent() == None → mkdir 不调用
    let b = backend_with_root_ctx();
    let w = b.open_write(&loc(), true).unwrap();
    let s = format!("{:?}", w);
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn copy_file_mkparents_with_root_skips_mkdir() {
    let b = backend_with_root_ctx();
    let n = b.copy_file(&loc(), &loc(), true).unwrap();
    assert_eq!(n, 5);
}

// ── copy_file write Err ───────────────────────────────────────

#[test]
fn copy_file_write_err_propagates() {
    let mut c = DummyClient::default();
    c.write_err = Some(io::ErrorKind::TimedOut);
    let b = backend_with_client(c);
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

// ── RemoteAdapter::map_error 默认透传 ─────────────────────────

#[test]
fn default_map_error_passthrough() {
    // DummyAdapter 不覆写 map_error，应透传
    let e = io::Error::new(io::ErrorKind::Other, "test");
    let mapped = DummyAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
}