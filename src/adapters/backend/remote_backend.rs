//! `RemoteBackend` 的 `Debug` + [`Backend`] trait 实现。
//!
//! 从 `remote.rs` 拆出（原 514 行 → ≤300）：`Backend` trait 全部 12 个方法；泛型骨架
//!（递归 walk / 递归 mkdir / 统一错误日志 / 缓冲写入）见 `remote_walk` / `remote_writer`。

use std::io;
use std::sync::Arc;

use crate::entities::backend::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

use super::{MAX_TEXT_BYTES, RemoteAdapter, RemoteBackend, RemoteBufferedWriter, RemoteTarget};
use super::{map_and_log, mkdir_recursive, mkparent, walk_recursive};

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
        // 与 LocalBackend WalkBuilder 同口径递归扫描子目录：单层 list 会让
        // SMB/ADB/MTP source 下子目录的全部媒体文件被 visit_location 静默丢失
        //（visit 仅消费 EntryKind::File，Dir entry 不会被递归驱动）。
        // 远端 list 是同步 IO，eager 收集所有 entry 后一次性返回——sources 实测
        // ≤ 数万文件，远小于 hash/EXIF 阶段的内存峰值，无需引入懒迭代复杂度。
        let mut out: Vec<io::Result<Entry>> = Vec::new();
        walk_recursive::<A>(&self.adapter, &target, &mut out);
        Box::new(out.into_iter())
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = self.build_target(loc)?;
        let bytes = self
            .adapter
            .client()
            .read(&target)
            .map_err(|e| map_and_log(A::scheme(), "read", target.path(), A::map_error, e))?;
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
        // TODO(perf): 当前是 read(整文件到本地) + write(整文件回远端) 两次全量 RTT +
        // 全文件堆分配。pavao 支持 SMB2 FSCTL_SRV_COPYCHUNK 服务端复制（零字节回
        // 客户端），adb 同设备可走 `shell cp /sdcard/A /sdcard/B`，libmtp 有
        // MoveObject API——同 scheme 同 host 场景理想上应先试 server-side copy，
        // 失败再 fallback 到 read+write。改动依赖 RemoteClient trait 加
        // `server_side_copy` 方法 + 每 adapter 实现，非本次重构范围。
        let src_target = self.build_target(src)?;
        let dst_target = self.build_target(dst)?;
        if mkparents {
            mkparent::<A>(&dst_target, self.adapter.client());
        }
        let bytes =
            self.adapter.client().read(&src_target).map_err(|e| {
                map_and_log(A::scheme(), "read", src_target.path(), A::map_error, e)
            })?;
        self.adapter
            .client()
            .write(&dst_target, &bytes)
            .map_err(|e| map_and_log(A::scheme(), "write", dst_target.path(), A::map_error, e))
    }
}
