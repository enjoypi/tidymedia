//! `RemoteBackend` 的 `Debug` + [`Backend`] trait 实现。
//!
//! 从 `remote.rs` 拆出（原 514 行 → ≤300）：`Backend` trait 全部 12 个方法；泛型骨架
//!（递归 walk / 递归 mkdir / 统一错误日志 / 缓冲写入）见 `remote_walk` / `remote_writer`。

use std::io;

use camino::Utf8PathBuf;

use crate::entities::backend::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

use super::{MAX_TEXT_BYTES, RemoteAdapter, RemoteBackend, RemoteTarget};
use super::{map_and_log, mkdir_recursive, mkparent, walk_recursive};

struct RemoteWriter {
    inner: Box<dyn MediaWriter>,
    path: Utf8PathBuf,
    scheme: &'static str,
    map_error: fn(io::Error) -> io::Error,
}

impl RemoteWriter {
    fn new<A: RemoteAdapter>(inner: Box<dyn MediaWriter>, target: &A::Target) -> Self {
        Self {
            inner,
            path: target.path().to_path_buf(),
            scheme: A::scheme(),
            map_error: A::map_error,
        }
    }

    fn map(&self, operation: &'static str, error: io::Error) -> io::Error {
        map_and_log(self.scheme, operation, &self.path, self.map_error, error)
    }
}

impl std::fmt::Debug for RemoteWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl io::Write for RemoteWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf).map_err(|e| self.map("write", e))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().map_err(|e| self.map("flush", e))
    }
}

impl MediaWriter for RemoteWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        let Self {
            inner,
            path,
            scheme,
            map_error,
        } = *self;
        inner
            .finish()
            .map_err(|e| map_and_log(scheme, "finish", &path, map_error, e))
    }
}

impl<A: RemoteAdapter> std::fmt::Debug for RemoteBackend<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBackend")
            .field("scheme", &A::scheme())
            // adapter 含 Arc<dyn Client>，不 impl Debug，故用 finish_non_exhaustive
            .finish_non_exhaustive()
    }
}

impl<A: RemoteAdapter> Backend for RemoteBackend<A> {
    fn scheme(&self) -> &'static str {
        A::scheme()
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let target = self.build_target(loc)?;
        self.adapter
            .client()
            .stat(&target)
            .map_err(|e| map_and_log(A::scheme(), "stat", target.path(), A::map_error, e))
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
        let mut out: Vec<io::Result<Entry>> = Vec::new();
        walk_recursive::<A>(&self.adapter, &target, &mut out);
        Box::new(out.into_iter())
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = self.build_target(loc)?;
        self.adapter
            .client()
            .open_reader(&target)
            .map_err(|e| map_and_log(A::scheme(), "open_reader", target.path(), A::map_error, e))
    }

    fn open_write(&self, loc: &Location, mkparents: bool) -> io::Result<Box<dyn MediaWriter>> {
        let target = self.build_target(loc)?;
        if mkparents {
            mkparent::<A>(&target, self.adapter.client());
        }
        let writer = self.adapter.client().create_writer(&target).map_err(|e| {
            map_and_log(A::scheme(), "create_writer", target.path(), A::map_error, e)
        })?;
        Ok(Box::new(RemoteWriter::new::<A>(writer, &target)))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let target = self.build_target(loc)?;
        self.adapter
            .client()
            .unlink(&target)
            .map_err(|e| map_and_log(A::scheme(), "unlink", target.path(), A::map_error, e))
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let target = self.build_target(loc)?;
        mkdir_recursive::<A>(&target, self.adapter.client())
            .map_err(|e| map_and_log(A::scheme(), "mkdir", target.path(), A::map_error, e))
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let target = self.build_target(loc)?;
        // 远端 client.read 一次性把整文件入堆；read_to_string 唯一调用方是 sidecar
        // 发现（XMP / Takeout JSON），典型 < 10 KiB。先 stat 做大小封顶，防止
        // 不受信远端共享上一个 N GB 的 .json/.xmp 拖爆进程内存。
        let meta = self
            .adapter
            .client()
            .stat(&target)
            .map_err(|e| map_and_log(A::scheme(), "stat", target.path(), A::map_error, e))?;
        if meta.size > MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "remote text file too large: {} bytes (limit {MAX_TEXT_BYTES})",
                    meta.size
                ),
            ));
        }
        let bytes = self
            .adapter
            .client()
            .read(&target)
            .map_err(|e| map_and_log(A::scheme(), "read", target.path(), A::map_error, e))?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(&self, src: &Location, dst: &Location, mkparents: bool) -> io::Result<u64> {
        self.build_target(src)?;
        let dst_target = self.build_target(dst)?;
        if mkparents {
            mkparent::<A>(&dst_target, self.adapter.client());
        }
        crate::entities::backend::stream_copy(self, src, self, dst, false)
    }
}
