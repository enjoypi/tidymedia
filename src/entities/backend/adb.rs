//! ADB Backend：把 `adb://[serial]/abs/path` 形态的 [`Location`] 转成 [`AdbClient`]
//! trait 上的调用。client 自身可注入；真实库适配器走 `adb_client` crate（feature gated）。
//!
//! ## URI 语义
//! - `adb://EMULATOR5554/sdcard/DCIM` —— 指定 serial 选定设备
//! - `adb:///sdcard/DCIM`             —— serial 为空，由 client 自动选择唯一在线设备
//! - path 始终是设备上的绝对路径（以 `/` 开头）
//!
//! ## 调度边界
//! - 单元测试通过 [`AdbBackend::with_client`] 注入 fake 验证：URI 解析、Target 翻译、
//!   error 映射等调度逻辑可在不依赖 adb daemon / Android 设备的前提下 100% 覆盖
//! - 真实 IO 由 [`real::RealAdbClient`] 在 `--features adb-backend` 启用时接入；
//!   该模块整体 `coverage(off)`（需真机 + adb-server 才能稳定触发，CI 不可覆盖）
//!
//! ## 协议限制
//! - adb sync 协议原生只暴露 stat / list / pull(read) / push(write)，**无原生 unlink / mkdir**；
//!   [`real::RealAdbClient`] 通过 `shell_command("rm -f ...")` / `shell_command("mkdir -p ...")`
//!   补齐。shell 参数走单引号转义防注入（见 [`shell_quote`]）。
//! - serial 模糊匹配：URI 内是 `adb devices` 列出的精确 serial；本 backend 不做 fuzzy
//!   匹配（MTP 那种 device_match=fuzzy 在 USB serial 场景不适用）

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

/// ADB target 的最小可识别参数集。`AdbClient` 实现按此参数访问设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdbTarget {
    /// 设备 serial；`None` 表示 client 自动选择唯一在线设备
    pub serial: Option<String>,
    /// 设备上的绝对路径（以 `/` 开头）
    pub path: Utf8PathBuf,
}

/// ADB 协议客户端抽象。`AdbBackend` 持 `Arc<dyn AdbClient>` 让真实库适配器与
/// fake 测试 client 等价替换。
pub trait AdbClient: Send + Sync + std::fmt::Debug {
    fn stat(&self, target: &AdbTarget) -> io::Result<Metadata>;
    fn list(&self, target: &AdbTarget) -> io::Result<Vec<Entry>>;
    fn read(&self, target: &AdbTarget) -> io::Result<Vec<u8>>;
    fn write(&self, target: &AdbTarget, data: &[u8]) -> io::Result<u64>;
    fn unlink(&self, target: &AdbTarget) -> io::Result<()>;
    fn mkdir(&self, target: &AdbTarget) -> io::Result<()>;
}

pub struct AdbBackend {
    client: Arc<dyn AdbClient>,
}

impl AdbBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported` 让上层报错。
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "adb-backend not enabled; rebuild with --features adb-backend",
        ))
    }

    /// 注入自定义 client。测试用 fake；生产路径由 `lib.rs::build_adb_backend`
    /// 在 feature 启用时构造 `RealAdbClient` 后注入。
    pub fn with_client(client: Arc<dyn AdbClient>) -> Self {
        Self { client }
    }

    /// `Arc<dyn Backend>` 工厂：方便 `DefaultBackendFactory::for_location` 装配。
    pub fn arc_with_client(client: Arc<dyn AdbClient>) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client))
    }
}

impl std::fmt::Debug for AdbBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbBackend")
            .field("client", &self.client)
            .finish()
    }
}

impl Backend for AdbBackend {
    fn scheme(&self) -> &'static str {
        "adb"
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let target = build_target(loc)?;
        self.client.stat(&target).map_err(map_adb_error)
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        match self.metadata(loc) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        let target = match build_target(root) {
            Ok(t) => t,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let entries = match self.client.list(&target).map_err(map_adb_error) {
            Ok(v) => v,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        Box::new(entries.into_iter().map(Ok))
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = build_target(loc)?;
        let bytes = self.client.read(&target).map_err(map_adb_error)?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    fn open_write(
        &self,
        loc: &Location,
        mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>> {
        let target = build_target(loc)?;
        if mkparents {
            if let Some(parent) = parent_target(&target) {
                let _ = self.client.mkdir(&parent);
            }
        }
        Ok(Box::new(AdbBufferedWriter {
            target,
            client: Arc::clone(&self.client),
            buffer: Vec::new(),
        }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let target = build_target(loc)?;
        self.client.unlink(&target).map_err(map_adb_error)
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let target = build_target(loc)?;
        self.client.mkdir(&target).map_err(map_adb_error)
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let bytes = {
            let target = build_target(loc)?;
            self.client.read(&target).map_err(map_adb_error)?
        };
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        mkparents: bool,
    ) -> io::Result<u64> {
        let src_target = build_target(src)?;
        let dst_target = build_target(dst)?;
        if mkparents {
            if let Some(p) = parent_target(&dst_target) {
                let _ = self.client.mkdir(&p);
            }
        }
        let bytes = self.client.read(&src_target).map_err(map_adb_error)?;
        self.client.write(&dst_target, &bytes).map_err(map_adb_error)
    }
}

/// ADB 写句柄：buffer 暂存 write 调用，`finish` 时一次性 push 给 client。
/// adb sync push 协议本身不支持流式追加，必须一次性整体上传。
struct AdbBufferedWriter {
    target: AdbTarget,
    client: Arc<dyn AdbClient>,
    buffer: Vec<u8>,
}

impl std::fmt::Debug for AdbBufferedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbBufferedWriter")
            .field("target", &self.target)
            .field("buffered_bytes", &self.buffer.len())
            .finish()
    }
}

impl io::Write for AdbBufferedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MediaWriter for AdbBufferedWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.client
            .write(&self.target, &self.buffer)
            .map(|_| ())
            .map_err(map_adb_error)
    }
}

/// 从 [`Location`] 解出 [`AdbTarget`]。非 ADB scheme 返回 [`io::ErrorKind::InvalidInput`]。
pub(crate) fn build_target(loc: &Location) -> io::Result<AdbTarget> {
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

/// 取目标的父目录。若 path 已是 `/` 根（无父），返回 None 让上层不去 mkdir 根。
pub(crate) fn parent_target(t: &AdbTarget) -> Option<AdbTarget> {
    let parent = t.path.parent()?;
    // Utf8Path::parent("/x") == Some("/")；parent("/") == None。
    // 设备根 `/` 不应该被 mkdir，所以再过滤一次。
    if parent.as_str().is_empty() || parent.as_str() == "/" {
        return None;
    }
    Some(AdbTarget {
        path: parent.to_path_buf(),
        ..t.clone()
    })
}

/// 把 adb_client 报错文案中的常见特征字符串映射成 [`io::ErrorKind`]：
/// - 含 "no such file" / "does not exist" → `NotFound`
/// - 含 "permission" → `PermissionDenied`
/// - 含 "device not found" / "no devices" → `NotFound`（serial 不匹配 / 无在线设备）
/// - 其他错误透传
///
/// 真实 adb_client 多用 [`io::Error::other`] 包错误码，文案检测足够覆盖单测 +
/// 简单远端场景；future PR 接入复杂场景再扩展。
pub(crate) fn map_adb_error(e: io::Error) -> io::Error {
    if e.kind() != io::ErrorKind::Other {
        return e;
    }
    let msg = e.to_string().to_lowercase();
    if msg.contains("no such file") || msg.contains("does not exist") {
        return io::Error::new(io::ErrorKind::NotFound, e.to_string());
    }
    if msg.contains("permission") {
        return io::Error::new(io::ErrorKind::PermissionDenied, e.to_string());
    }
    if msg.contains("device not found") || msg.contains("no devices") {
        return io::Error::new(io::ErrorKind::NotFound, e.to_string());
    }
    e
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
