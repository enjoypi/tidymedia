//! 远端 Backend 统一抽象层。
//!
//! [`RemoteTarget`] / [`RemoteClient`] / [`RemoteAdapter`] 三个 trait 把 SMB / ADB / MTP
//! 三套 90% 同构的 Backend 实现收敛到一个泛型 [`RemoteBackend<A>`] 上，消除 ~600 行
//! 重复骨架代码。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use crate::entities::backend::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

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

    /// 协议级错误映射。默认透传；SMB/ADB 覆写以识别 EACCES / NotFound 等文案。
    #[allow(unused_variables)]
    fn map_error(e: io::Error) -> io::Error {
        e
    }

    /// 协议上下文引用。用于 `Target::from_location`。
    fn ctx(&self) -> &<Self::Target as RemoteTarget>::Ctx;

    /// client 句柄引用。
    fn client(&self) -> &Arc<dyn RemoteClient<Self::Target>>;
}

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

fn mkparent<A: RemoteAdapter>(target: &A::Target, client: &Arc<dyn RemoteClient<A::Target>>) {
    if let Some(parent) = target.parent() {
        let _ = client.mkdir(&parent);
    }
}

impl<A: RemoteAdapter> std::fmt::Debug for RemoteBackend<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBackend")
            .field("scheme", &A::scheme())
            .finish()
    }
}

impl<A: RemoteAdapter> Backend for RemoteBackend<A> {
    fn scheme(&self) -> &'static str {
        A::scheme()
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let target = self.build_target(loc)?;
        self.adapter.client().stat(&target).map_err(A::map_error)
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
        let target = match self.build_target(root) {
            Ok(t) => t,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let entries = match self.adapter.client().list(&target).map_err(A::map_error) {
            Ok(v) => v,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        Box::new(entries.into_iter().map(Ok))
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = self.build_target(loc)?;
        let bytes = self.adapter.client().read(&target).map_err(A::map_error)?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    fn open_write(&self, loc: &Location, mkparents: bool) -> io::Result<Box<dyn MediaWriter>> {
        let target = self.build_target(loc)?;
        if mkparents {
            mkparent::<A>(&target, self.adapter.client());
        }
        Ok(Box::new(RemoteBufferedWriter::<A> {
            target,
            client: Arc::clone(self.adapter.client()),
            buffer: Vec::new(),
        }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let target = self.build_target(loc)?;
        self.adapter.client().unlink(&target).map_err(A::map_error)
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let target = self.build_target(loc)?;
        self.adapter.client().mkdir(&target).map_err(A::map_error)
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let bytes = {
            let target = self.build_target(loc)?;
            self.adapter.client().read(&target).map_err(A::map_error)?
        };
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(&self, src: &Location, dst: &Location, mkparents: bool) -> io::Result<u64> {
        let src_target = self.build_target(src)?;
        let dst_target = self.build_target(dst)?;
        if mkparents {
            mkparent::<A>(&dst_target, self.adapter.client());
        }
        let bytes = self
            .adapter
            .client()
            .read(&src_target)
            .map_err(A::map_error)?;
        self.adapter
            .client()
            .write(&dst_target, &bytes)
            .map_err(A::map_error)
    }
}

pub(crate) struct RemoteBufferedWriter<A: RemoteAdapter> {
    target: A::Target,
    client: Arc<dyn RemoteClient<A::Target>>,
    buffer: Vec<u8>,
}

impl<A: RemoteAdapter> std::fmt::Debug for RemoteBufferedWriter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBufferedWriter")
            .field("target", &self.target)
            .field("buffered_bytes", &self.buffer.len())
            .finish()
    }
}

impl<A: RemoteAdapter> io::Write for RemoteBufferedWriter<A> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<A: RemoteAdapter> MediaWriter for RemoteBufferedWriter<A> {
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.client
            .write(&self.target, &self.buffer)
            .map(|_| ())
            .map_err(A::map_error)
    }
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
