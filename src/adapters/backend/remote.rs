//! 远端 Backend 统一抽象层。
//!
//! [`RemoteTarget`] / [`RemoteClient`] / [`RemoteAdapter`] 三个 trait 把 SMB / ADB / MTP
//! 三套 90% 同构的 Backend 实现收敛到一个泛型 [`RemoteBackend<A>`] 上，消除 ~600 行
//! 重复骨架代码。
//!
//! 拆分为四文件（原 514 行 → ≤300）：`remote_walk`（递归 walk/mkdir/统一错误日志）、
//! `remote_writer`（`RemoteBufferedWriter` IO impl + 上限守卫）、`remote_backend`
//!（`RemoteBackend` 的 `Debug` + `Backend` 实现）、本文件（trait 面 + 常量 + 错误映射 + 结构定义）。

#[path = "remote_backend.rs"]
mod backend_impl;
#[path = "remote_walk.rs"]
mod walk;
#[path = "remote_writer.rs"]
mod writer;

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use crate::entities::backend::{Entry, Metadata};
use crate::entities::uri::Location;

#[cfg(test)]
use crate::entities::backend::{Backend, MediaWriter};
use walk::{map_and_log, mkdir_recursive, mkparent, walk_recursive};
#[cfg(test)]
use writer::check_buffer_size;

/// 远端存储目标的协议相关参数（host/path/凭据等）。
/// 每个远端协议实现自己的 Target 类型（`SmbTarget` / `AdbTarget` / `MtpTarget`）。
pub trait RemoteTarget: Clone + Send + Sync + std::fmt::Debug + Eq + 'static {
    /// 协议相关的上下文类型：SMB/ADB 为 `()`，MTP 为 `(MtpMatch, MtpMatch)`。
    type Ctx: Send + Sync + 'static;

    /// 从 [`Location`] + 协议上下文解出 target；scheme 不匹配时返回
    /// [`io::ErrorKind::InvalidInput`]。
    fn from_location(loc: &Location, ctx: &Self::Ctx) -> io::Result<Self>;

    /// 父目录 target；已在根则返回 `None`。
    fn parent(&self) -> Option<Self>;

    /// 反向构造：给定此 target 下的子路径，构造对应的 [`Location`]。
    fn entry_location(&self, path: Utf8PathBuf) -> Location;

    /// target 内的路径部分（不含 scheme/host/share 等前缀）。
    fn path(&self) -> &camino::Utf8Path;
}

/// 远端协议客户端的 6 个基础 IO 操作。实现者可以是真实库适配器（如
/// `RealSmbClient`）或测试用 fake。
///
/// # 已知限制：`read` 整文件入堆
///
/// `read` 返回 `Vec<u8>`，`open_read` / `copy_file` 因而把远端文件完整读入内存
///（大视频在内存受限环境有 OOM 风险）。这是依赖层形态决定的：pavao 的
/// `SmbFile` 借用 client 生命周期，无法装入 `Box<dyn MediaReader + 'static>`；
/// `adb_client` 的 pull 是回调式写入 API。真正流式需要换库或线程+管道泵数据，
/// 当前按 YAGNI 不做（CLAUDE.md「项目 Gotcha」有记录）。
pub trait RemoteClient<T: RemoteTarget>: Send + Sync + std::fmt::Debug {
    fn stat(&self, t: &T) -> io::Result<Metadata>;
    fn list(&self, t: &T) -> io::Result<Vec<Entry>>;
    fn read(&self, t: &T) -> io::Result<Vec<u8>>;
    fn write(&self, t: &T, data: &[u8]) -> io::Result<u64>;
    fn unlink(&self, t: &T) -> io::Result<()>;
    fn mkdir(&self, t: &T) -> io::Result<()>;
}

/// 把 Target + Client + scheme + error 映射捆成一个适配器。
/// [`RemoteBackend<A>`] 通过此 trait 获得协议相关参数，自身保持完全泛型。
pub trait RemoteAdapter: Send + Sync + 'static {
    type Target: RemoteTarget;

    /// Backend scheme 字符串（`"smb"` / `"adb"` / `"mtp"`）。
    fn scheme() -> &'static str;

    /// 协议级错误映射。默认透传；SMB/ADB 覆写以识别 EACCES / `NotFound` 等文案。
    #[allow(unused_variables)]
    fn map_error(e: io::Error) -> io::Error {
        e
    }

    /// 协议上下文引用。用于 `Target::from_location`。
    fn ctx(&self) -> &<Self::Target as RemoteTarget>::Ctx;

    /// client 句柄引用。
    fn client(&self) -> &Arc<dyn RemoteClient<Self::Target>>;
}

/// feature off 时各 `RemoteBackend` 别名共用的 `Unsupported` 构造：把 scheme + feature
/// 名拼成与 factory 路径同口径的错误文案，让用户看到的提示是统一的"<scheme>-backend
/// not enabled; rebuild with --features <scheme>-backend"。
#[must_use]
pub fn unsupported_backend(feature: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{feature} not enabled; rebuild with --features {feature}"),
    )
}

/// sidecar 文本文件大小上限（防 `OOM`）：`sidecar.rs::read_to_string` 唯一消费者，
/// XMP / Takeout JSON 实测 < 10 KiB；8 MiB 给极端 Takeout 复合 export 留足空间，
/// 同时把恶意/损坏 sidecar 文件的内存放大封顶在常数倍。Local/Remote 共用同一上限：
/// 不受信媒体目录（USB/SD/网盘挂载）下注入 1 GB 假 `.xmp` 可让进程 OOM，与远端
/// 共享上限是同口径防御。
pub(crate) const MAX_TEXT_BYTES: u64 = 8 << 20;

/// `RemoteBufferedWriter` 缓冲上限：远端 `client.write` 是"一次性把整个 buffer 提交"
/// 语义（pavao `SmbFile` / adb push / libmtp 都不支持真流式），单文件必须整体入堆。
/// 2 GiB 上限让 Android FFI (feature `android-app`, 2–4 GB RAM) 场景 fail-fast 而非
/// 静默 OOM 崩进程；桌面通常 RAM 充足此值不触发。极端超大文件（4K 全片视频 >2 GiB）
/// 会 fail 但比默默死机可诊断。CLAUDE.md 例外「IO 边界常量」类，不外置到 config。
pub(crate) const MAX_REMOTE_WRITE_BUFFER: u64 = 2 << 30;

/// 泛型远端 Backend：对任意 [`RemoteAdapter`] 实现 [`Backend`] trait 的全部 12 个方法。
/// SMB / ADB / MTP 三套实现收敛到此单一泛型。
pub struct RemoteBackend<A: RemoteAdapter> {
    pub(crate) adapter: A,
}

impl<A: RemoteAdapter> RemoteBackend<A> {
    fn build_target(&self, loc: &Location) -> io::Result<A::Target> {
        A::Target::from_location(loc, self.adapter.ctx())
    }
}

/// 远端 client 错误的通用文案重映射（SMB/ADB 共用骨架）。
///
/// 先放行已正确分类的 `NotFound` / `PermissionDenied`（防 future client 版本提前
/// 归一时被重复包装丢失分类），再按 ASCII 文案重新归类 `Other` / `BrokenPipe` /
/// `ConnectionReset` 等链式错误。`extra_not_found` 让方言注入额外 `NotFound`
/// 触发文案（ADB 的 `"device not found"` / `"no devices"` 等）。
/// `to_ascii_lowercase` 比 `to_lowercase` 快且不做 Unicode full-case folding，
/// 避免 `contains` 在本地化消息上字节漂移让匹配丢失。
pub(crate) fn map_remote_error(e: io::Error, extra_not_found: &[&str]) -> io::Error {
    if matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    ) {
        return e;
    }
    let msg = e.to_string().to_ascii_lowercase();
    if msg.contains("enoent")
        || msg.contains("no such file")
        || msg.contains("does not exist")
        || extra_not_found.iter().any(|s| msg.contains(s))
    {
        return io::Error::new(io::ErrorKind::NotFound, e.to_string());
    }
    if msg.contains("eacces") || msg.contains("permission") {
        return io::Error::new(io::ErrorKind::PermissionDenied, e.to_string());
    }
    // 并发 mkdir / 重复创建：client 可能把 EEXIST 包成 Other("File exists") 等文案；
    // mkdir_recursive 的 AlreadyExists 容忍依赖此重映射，否则多 source 并发归档
    // 到同 {year}/{month} 桶时硬失败。
    if msg.contains("eexist") || msg.contains("file exists") || msg.contains("already exists") {
        return io::Error::new(io::ErrorKind::AlreadyExists, e.to_string());
    }
    e
}

pub(crate) struct RemoteBufferedWriter<A: RemoteAdapter> {
    target: A::Target,
    client: Arc<dyn RemoteClient<A::Target>>,
    buffer: Vec<u8>,
}

#[cfg(test)]
#[path = "remote_test_helpers.rs"]
mod test_helpers;

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "remote_advanced_tests.rs"]
mod advanced_tests;
