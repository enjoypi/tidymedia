//! `remote.rs` 测试共享 helpers：`DummyTarget` / `DummyClient` / `DummyAdapter` +
//! `backend()` / `loc()` / `backend_with_client` / `backend_with_from_loc_err`。
//! 从 `remote_tests.rs` 抽出避免单文件 > 512 行（P0 §6），同时让 IO 与 advanced 测试共用。
//! 业务覆盖由 `remote_tests.rs` / `remote_advanced_tests.rs` 通过 helper 调用透传到 `remote.rs`。

use std::io;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use super::*;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

// ── DummyTarget ──────────────────────────────────────────────

/// Ctx 携带可选的 `from_location` 注入错误和路径覆写。
#[derive(Clone, Debug)]
pub(super) struct DummyCtx {
    from_loc_err: Option<io::ErrorKind>,
    /// 覆写 `from_location` 返回的路径；None 时默认 "/dummy"
    path_override: Option<Utf8PathBuf>,
}

impl DummyCtx {
    pub(super) fn ok_with_path(p: &str) -> Self {
        Self {
            from_loc_err: None,
            path_override: Some(Utf8PathBuf::from(p)),
        }
    }
    pub(super) fn ok() -> Self {
        Self {
            from_loc_err: None,
            path_override: None,
        }
    }
    pub(super) fn with_err(kind: io::ErrorKind) -> Self {
        Self {
            from_loc_err: Some(kind),
            path_override: None,
        }
    }
    pub(super) fn with_root_path() -> Self {
        Self {
            from_loc_err: None,
            path_override: Some(Utf8PathBuf::from("/")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DummyTarget {
    path: Utf8PathBuf,
    /// 为测试 `parent()` == None 设根路径
    is_root: bool,
}

impl DummyTarget {
    pub(super) fn new(path: &str) -> Self {
        Self {
            path: Utf8PathBuf::from(path),
            is_root: false,
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
        Location::Local(p)
    }

    fn path(&self) -> &Utf8Path {
        &self.path
    }
}

// ── DummyClient（按方法注入 ErrorKind）────────────────────────

#[derive(Debug, Default)]
pub(super) struct DummyClient {
    pub(super) stat: Option<io::ErrorKind>,
    pub(super) list: Option<io::ErrorKind>,
    pub(super) read: Option<io::ErrorKind>,
    pub(super) write: Option<io::ErrorKind>,
    pub(super) unlink: Option<io::ErrorKind>,
    pub(super) mkdir: Option<io::ErrorKind>,
    /// mkdir 实际被调用的次数；杀「mkparent 整函数被替换成 ()」类变异用。
    pub(super) mkdir_calls: std::sync::atomic::AtomicUsize,
}

impl DummyClient {
    pub(super) fn with_stat_err(k: io::ErrorKind) -> Self {
        Self {
            stat: Some(k),
            ..Default::default()
        }
    }
    pub(super) fn with_list_err(k: io::ErrorKind) -> Self {
        Self {
            list: Some(k),
            ..Default::default()
        }
    }
    pub(super) fn with_read_err(k: io::ErrorKind) -> Self {
        Self {
            read: Some(k),
            ..Default::default()
        }
    }
}

impl RemoteClient<DummyTarget> for DummyClient {
    fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
        if let Some(k) = self.stat {
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
        if let Some(k) = self.list {
            return Err(io::Error::from(k));
        }
        Ok(vec![])
    }
    fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
        if let Some(k) = self.read {
            return Err(io::Error::from(k));
        }
        Ok(b"hello".to_vec())
    }
    fn write(&self, _t: &DummyTarget, data: &[u8]) -> io::Result<u64> {
        if let Some(k) = self.write {
            return Err(io::Error::from(k));
        }
        Ok(data.len() as u64)
    }
    fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
        if let Some(k) = self.unlink {
            return Err(io::Error::from(k));
        }
        Ok(())
    }
    fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
        self.mkdir_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(k) = self.mkdir {
            return Err(io::Error::from(k));
        }
        Ok(())
    }
}

// ── DummyAdapter ─────────────────────────────────────────────

pub(super) struct DummyAdapter {
    pub(super) client: Arc<dyn RemoteClient<DummyTarget>>,
    ctx: DummyCtx,
}

impl DummyAdapter {
    pub(super) fn with_client(client: Arc<dyn RemoteClient<DummyTarget>>) -> Self {
        Self {
            client,
            ctx: DummyCtx::ok(),
        }
    }
    pub(super) fn with_client_and_ctx(
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

pub(super) fn backend() -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend {
        adapter: DummyAdapter::with_client(client),
    }
}

pub(super) fn loc() -> Location {
    Location::Local("/dummy".into())
}

pub(super) fn backend_with_client(c: DummyClient) -> RemoteBackend<DummyAdapter> {
    RemoteBackend {
        adapter: DummyAdapter::with_client(Arc::new(c)),
    }
}

pub(super) fn backend_with_from_loc_err(kind: io::ErrorKind) -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend {
        adapter: DummyAdapter::with_client_and_ctx(client, DummyCtx::with_err(kind)),
    }
}

pub(super) fn backend_with_root_ctx() -> RemoteBackend<DummyAdapter> {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    RemoteBackend {
        adapter: DummyAdapter::with_client_and_ctx(client, DummyCtx::with_root_path()),
    }
}
