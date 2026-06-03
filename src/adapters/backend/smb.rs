//! SMB Backend：把 `smb://[user@]host[:port]/share/path` 形态的 [`Location`]
//! 转成 [`SmbClient`] trait 上的调用，client 自身可注入。
//!
//! 本模块已迁到 [`RemoteBackend`] 泛型骨架：`SmbBackend` 是
//! `RemoteBackend<SmbAdapter>` 的类型别名，Backend trait 实现由 generic 层提供。
//!
//! ## 环境变量
//! - `SMB_PASSWORD`：URI 中没有 `user@` 时由 `default_user` 兜底；密码总是经环境变量
//! - `KRB5CCNAME`：Kerberos ticket cache 路径（让 client 走 SSO）
//!
//! 详见 CLAUDE.md「URI 与 Backend」段。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::remote::{RemoteAdapter, RemoteBackend, RemoteClient, RemoteTarget};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

/// SMB target 的最小可识别参数集。`SmbClient` 实现按此参数访问远端。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub share: String,
    pub path: Utf8PathBuf,
    /// 经环境变量 `SMB_PASSWORD` 注入；URI 中不暴露明文密码。
    pub password: Option<String>,
    /// Kerberos ticket cache 路径，对应环境变量 `KRB5CCNAME`。
    pub krb5_ccname: Option<String>,
}

impl RemoteTarget for SmbTarget {
    type Ctx = ();

    fn from_location(loc: &Location, _ctx: &()) -> io::Result<Self> {
        let Location::Smb {
            user,
            host,
            port,
            share,
            path,
        } = loc
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("SmbBackend cannot handle scheme {:?}", loc.scheme()),
            ));
        };
        Ok(SmbTarget {
            user: user.clone(),
            host: host.clone(),
            port: *port,
            share: share.clone(),
            path: path.clone(),
            password: std::env::var("SMB_PASSWORD").ok(),
            krb5_ccname: std::env::var("KRB5CCNAME").ok(),
        })
    }

    fn parent(&self) -> Option<Self> {
        let parent = self.path.parent()?;
        if parent.as_str().is_empty() {
            return None;
        }
        Some(SmbTarget {
            path: parent.to_path_buf(),
            ..self.clone()
        })
    }

    fn entry_location(&self, path: Utf8PathBuf) -> Location {
        Location::Smb {
            user: self.user.clone(),
            host: self.host.clone(),
            port: self.port,
            share: self.share.clone(),
            path,
        }
    }

    fn path(&self) -> &camino::Utf8Path {
        &self.path
    }
}

/// SMB 协议客户端抽象。公开别名：任何实现了 [`RemoteClient`]`<`[`SmbTarget`]`>` 的
/// 类型自动实现本 trait。
pub trait SmbClient: RemoteClient<SmbTarget> {}
impl<T: RemoteClient<SmbTarget>> SmbClient for T {}

/// SMB 适配器：把 [`SmbTarget`] + [`SmbClient`] + scheme + error 映射捆在一起，
/// 交给 [`RemoteBackend`] 泛型层驱动。
pub struct SmbAdapter {
    client: Arc<dyn RemoteClient<SmbTarget>>,
}

impl RemoteAdapter for SmbAdapter {
    type Target = SmbTarget;

    fn scheme() -> &'static str {
        "smb"
    }

    /// EACCES → PermissionDenied；其他 `ErrorKind` 透传。
    fn map_error(e: io::Error) -> io::Error {
        if e.kind() == io::ErrorKind::Other && e.to_string().contains("EACCES") {
            return io::Error::new(io::ErrorKind::PermissionDenied, e.to_string());
        }
        e
    }

    fn ctx(&self) -> &() {
        &()
    }

    fn client(&self) -> &Arc<dyn RemoteClient<SmbTarget>> {
        &self.client
    }
}

/// SMB Backend 类型别名。`Backend` trait 由泛型 [`RemoteBackend`]`<`[`SmbAdapter`]`>` 提供。
pub type SmbBackend = RemoteBackend<SmbAdapter>;

impl SmbBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported` 让上层报错。
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "smb-backend not enabled; rebuild with --features smb-backend",
        ))
    }

    /// 注入自定义 client。测试用 fake，生产路径注入 `RealSmbClient`。
    pub fn with_client(client: Arc<dyn SmbClient>) -> Self {
        let remote: Arc<dyn RemoteClient<SmbTarget>> = client;
        RemoteBackend {
            adapter: SmbAdapter { client: remote },
        }
    }

    /// `Arc<dyn Backend>` 工厂：方便 Registry / `DefaultBackendFactory::for_location`。
    pub fn arc_with_client(client: Arc<dyn SmbClient>) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client))
    }
}

#[cfg(test)]
#[path = "smb_tests.rs"]
mod tests;

#[cfg(feature = "smb-backend")]
#[path = "smb_real.rs"]
pub mod real;
