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

pub trait RemoteTarget: Clone + Send + Sync + std::fmt::Debug + Eq + 'static {
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

    fn scheme() -> &'static str;

    #[allow(unused_variables)]
    fn map_error(e: io::Error) -> io::Error {
        e
    }

    /// 协议上下文引用。用于 `Target::from_location`。
    fn ctx(&self) -> &<Self::Target as RemoteTarget>::Ctx;

    /// client 句柄引用。
    fn client(&self) -> &Arc<dyn RemoteClient<Self::Target>>;
}

/// sidecar 文本文件大小上限（防 `OOM`）：`sidecar.rs::read_to_string` 唯一消费者，
/// XMP / Takeout JSON 实测 < 10 KiB；8 MiB 给极端 Takeout 复合 export 留足空间，
/// 同时把恶意/损坏 sidecar 文件的内存放大封顶在常数倍。Local/Remote 共用同一上限：
/// 不受信媒体目录（USB/SD/网盘挂载）下注入 1 GB 假 `.xmp` 可让进程 OOM，与远端
/// 共享上限是同口径防御。
pub(crate) const MAX_TEXT_BYTES: u64 = 8 << 20;

pub(crate) const MAX_REMOTE_WRITE_BUFFER: u64 = 2 << 30;

pub struct RemoteBackend<A: RemoteAdapter> {
    pub(crate) adapter: A,
}

impl<A: RemoteAdapter> RemoteBackend<A> {
    fn build_target(&self, loc: &Location) -> io::Result<A::Target> {
        A::Target::from_location(loc, self.adapter.ctx())
    }
}

pub(crate) fn map_remote_error(e: io::Error) -> io::Error {
    if matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    ) {
        return e;
    }
    let msg = e.to_string().to_ascii_lowercase();
    if msg.contains("enoent") || msg.contains("no such file") || msg.contains("does not exist") {
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
#[path = "remote_fixture_tests.rs"]
mod test_helpers;

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "remote_advanced_tests.rs"]
mod advanced_tests;
