//! ADB Backend：把 `adb://[serial]/abs/path` 形态的 [`Location`] 转成 [`AdbClient`]
//! trait 上的调用。client 自身可注入；真实库适配器走 `adb_client` crate（feature gated）。
//!
//! ## URI 语义
//! - `adb://EMULATOR5554/sdcard/DCIM` —— 指定 serial 选定设备
//! - `adb:///sdcard/DCIM`             —— serial 为空，由 client 自动选择唯一在线设备
//! - path 始终是设备上的绝对路径（以 `/` 开头）
//!
//! 本模块已迁到 [`RemoteBackend`] 泛型骨架：`AdbBackend` 是
//! `RemoteBackend<AdbAdapter>` 的类型别名。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::remote::{RemoteAdapter, RemoteBackend, RemoteClient, RemoteTarget};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

/// ADB target 的最小可识别参数集。`AdbClient` 实现按此参数访问设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdbTarget {
    /// 设备 serial；`None` 表示 client 自动选择唯一在线设备
    pub serial: Option<String>,
    /// 设备上的绝对路径（以 `/` 开头）
    pub path: Utf8PathBuf,
}

impl RemoteTarget for AdbTarget {
    type Ctx = ();

    fn from_location(loc: &Location, _ctx: &()) -> io::Result<Self> {
        let Location::Adb { serial, path } = loc else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("AdbBackend cannot handle scheme {:?}", loc.scheme()),
            ));
        };
        Ok(AdbTarget {
            serial: serial.clone(),
            path: path.clone(),
        })
    }

    fn parent(&self) -> Option<Self> {
        let parent = self.path.parent()?;
        // Utf8Path::parent("/x") == Some("/")；parent("/") == None。
        // 设备根 `/` 不应该被 mkdir，所以再过滤一次。
        if parent.as_str().is_empty() || parent.as_str() == "/" {
            return None;
        }
        Some(AdbTarget {
            path: parent.to_path_buf(),
            ..self.clone()
        })
    }

    fn entry_location(&self, path: Utf8PathBuf) -> Location {
        Location::Adb {
            serial: self.serial.clone(),
            path,
        }
    }

    fn path(&self) -> &camino::Utf8Path {
        &self.path
    }
}

/// ADB 协议客户端抽象。公开别名：任何实现了 [`RemoteClient`]`<`[`AdbTarget`]`>` 的
/// 类型自动实现本 trait。
pub trait AdbClient: RemoteClient<AdbTarget> {}
impl<T: RemoteClient<AdbTarget>> AdbClient for T {}

/// ADB 适配器：把 [`AdbTarget`] + [`AdbClient`] + scheme + error 映射捆在一起，
/// 交给 [`RemoteBackend`] 泛型层驱动。
pub struct AdbAdapter {
    client: Arc<dyn RemoteClient<AdbTarget>>,
}

impl RemoteAdapter for AdbAdapter {
    type Target = AdbTarget;

    fn scheme() -> &'static str {
        "adb"
    }

    /// 把 adb_client 报错文案中的常见特征字符串映射成 [`io::ErrorKind`]。
    fn map_error(e: io::Error) -> io::Error {
        if e.kind() != io::ErrorKind::Other {
            return e;
        }
        let msg = e.to_string().to_lowercase();
        if msg.contains("no such file") || msg.contains("does not exist")
            || msg.contains("device not found") || msg.contains("no devices")
        {
            return io::Error::new(io::ErrorKind::NotFound, e.to_string());
        }
        if msg.contains("permission") {
            return io::Error::new(io::ErrorKind::PermissionDenied, e.to_string());
        }
        e
    }

    fn ctx(&self) -> &() {
        &()
    }

    fn client(&self) -> &Arc<dyn RemoteClient<AdbTarget>> {
        &self.client
    }
}

/// ADB Backend 类型别名。`Backend` trait 由泛型 [`RemoteBackend`]`<`[`AdbAdapter`]`>` 提供。
pub type AdbBackend = RemoteBackend<AdbAdapter>;

impl AdbBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported` 让上层报错。
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "adb-backend not enabled; rebuild with --features adb-backend",
        ))
    }

    /// 注入自定义 client。测试用 fake；生产路径由 lib.rs 在 feature 启用时
    /// 构造 `RealAdbClient` 后注入。
    pub fn with_client(client: Arc<dyn AdbClient>) -> Self {
        let remote: Arc<dyn RemoteClient<AdbTarget>> = client;
        RemoteBackend {
            adapter: AdbAdapter { client: remote },
        }
    }

    /// `Arc<dyn Backend>` 工厂：方便 `DefaultBackendFactory::for_location` 装配。
    pub fn arc_with_client(client: Arc<dyn AdbClient>) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client))
    }
}

/// 单引号封装一段字符串，让 adb shell 把它当成单参数；内部 `'` 通过 `'\''` 续接。
/// 用于在 [`real::RealAdbClient`] 的 `shell_command("rm -f ...")` 等调用上防注入。
/// `pub(crate)` 让 `adb_real.rs` 可见；feature off 时仅单元测试使用，故 cfg gate
/// 避免 dead_code 警告。
#[cfg(any(feature = "adb-backend", test))]
pub(crate) fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
#[path = "adb_tests.rs"]
mod tests;

#[cfg(feature = "adb-backend")]
#[path = "adb_real.rs"]
pub mod real;