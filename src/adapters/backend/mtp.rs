//! MTP Backend：把 `mtp://device/storage/path` 形态的 [`Location`]
//! 转成 [`MtpClient`] trait 上的调用。client 自身可注入，真实库适配器留作未来 PR。
//!
//! ## 设备 / 存储模糊匹配
//! - URI 内的 `device` / `storage` 字段是用户书写的"名字"（如 `Pixel 8` /
//!   `Internal shared storage`），与 MTP 协议层的 device id / storage id 不一定一一对应。
//! - 通过 [`MtpMatch`] 控制匹配语义：`Exact` 要求严格相等；`Fuzzy` 由 client 自决（通常
//!   走 `contains` 等模糊算法）。
//!
//! 本模块已迁到 [`RemoteBackend`] 泛型骨架：`MtpBackend` 是
//! `RemoteBackend<MtpAdapter>` 的类型别名。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::remote::{RemoteAdapter, RemoteBackend, RemoteClient, RemoteTarget};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

/// 匹配策略：与真实 MTP client 一起决定如何把 URI 内的 device/storage 名字
/// 落到协议层 id 上。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MtpMatch {
    Exact,
    Fuzzy,
}

/// MTP target 的最小可识别参数集。`MtpClient` 实现按此参数访问设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtpTarget {
    pub device: String,
    pub storage: String,
    pub path: Utf8PathBuf,
    pub device_match: MtpMatch,
    pub storage_match: MtpMatch,
}

impl RemoteTarget for MtpTarget {
    type Ctx = (MtpMatch, MtpMatch);

    fn from_location(loc: &Location, ctx: &(MtpMatch, MtpMatch)) -> io::Result<Self> {
        let Location::Mtp {
            device,
            storage,
            path,
        } = loc
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("MtpBackend cannot handle scheme {:?}", loc.scheme()),
            ));
        };
        Ok(MtpTarget {
            device: device.clone(),
            storage: storage.clone(),
            path: path.clone(),
            device_match: ctx.0,
            storage_match: ctx.1,
        })
    }

    fn parent(&self) -> Option<Self> {
        let parent = self.path.parent()?;
        if parent.as_str().is_empty() {
            return None;
        }
        Some(MtpTarget {
            path: parent.to_path_buf(),
            ..self.clone()
        })
    }

    fn entry_location(&self, path: Utf8PathBuf) -> Location {
        Location::Mtp {
            device: self.device.clone(),
            storage: self.storage.clone(),
            path,
        }
    }

    fn path(&self) -> &camino::Utf8Path {
        &self.path
    }
}

/// MTP 协议客户端抽象。公开别名：任何实现了 [`RemoteClient`]`<`[`MtpTarget`]`>` 的
/// 类型自动实现本 trait。
pub trait MtpClient: RemoteClient<MtpTarget> {}
impl<T: RemoteClient<MtpTarget>> MtpClient for T {}

/// MTP 适配器：把 [`MtpTarget`] + [`MtpClient`] + scheme + 匹配策略捆在一起，
/// 交给 [`RemoteBackend`] 泛型层驱动。`map_error` 为默认透传。
pub struct MtpAdapter {
    client: Arc<dyn RemoteClient<MtpTarget>>,
    matches: (MtpMatch, MtpMatch),
}

impl RemoteAdapter for MtpAdapter {
    type Target = MtpTarget;

    fn scheme() -> &'static str {
        "mtp"
    }

    // map_error 默认透传（与原 MtpBackend 一致）

    fn ctx(&self) -> &(MtpMatch, MtpMatch) {
        &self.matches
    }

    fn client(&self) -> &Arc<dyn RemoteClient<MtpTarget>> {
        &self.client
    }
}

/// MTP Backend 类型别名。`Backend` trait 由泛型 [`RemoteBackend`]`<`[`MtpAdapter`]`>` 提供。
pub type MtpBackend = RemoteBackend<MtpAdapter>;

impl MtpBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported`。
    pub fn new() -> io::Result<Self> {
        Err(super::remote::unsupported_backend("mtp-backend"))
    }

    /// 注入自定义 client + 匹配策略。
    pub fn with_client(
        client: Arc<dyn MtpClient>,
        device_match: MtpMatch,
        storage_match: MtpMatch,
    ) -> Self {
        let remote: Arc<dyn RemoteClient<MtpTarget>> = client;
        RemoteBackend {
            adapter: MtpAdapter {
                client: remote,
                matches: (device_match, storage_match),
            },
        }
    }

    /// `Arc<dyn Backend>` 工厂。
    pub fn arc_with_client(
        client: Arc<dyn MtpClient>,
        device_match: MtpMatch,
        storage_match: MtpMatch,
    ) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client, device_match, storage_match))
    }
}

#[cfg(test)]
#[path = "mtp_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mtp_types_tests.rs"]
mod types_tests;

#[cfg(feature = "mtp-backend")]
#[path = "mtp_real.rs"]
pub mod real;
