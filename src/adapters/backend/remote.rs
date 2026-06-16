//! 远端 Backend 统一抽象层。
//!
//! [`RemoteTarget`] / [`RemoteClient`] / [`RemoteAdapter`] 三个 trait 把 SMB / ADB / MTP
//! 三套 90% 同构的 Backend 实现收敛到一个泛型 [`RemoteBackend<A>`] 上，消除 ~600 行
//! 重复骨架代码。

use std::io;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use tracing::debug;

use crate::entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
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

/// 远端 sidecar 文本文件大小上限（防 `OOM`）：`sidecar.rs::read_to_string` 唯一消费者，
/// XMP / Takeout JSON 实测 < 10 KiB；8 MiB 给极端 Takeout 复合 export 留足空间，
/// 同时把恶意/损坏远端文件的内存放大封顶在常数倍。
pub(crate) const MAX_REMOTE_TEXT_BYTES: u64 = 8 << 20;

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

/// 统一记录远端 op 失败并套用协议级错误映射（R3：外部调用失败不静默）。
/// 非泛型：日志逻辑只编译一份，避免 monomorphization 覆盖率重复计数。
fn map_and_log(
    scheme: &'static str,
    operation: &'static str,
    path: &Utf8Path,
    map: fn(io::Error) -> io::Error,
    e: io::Error,
) -> io::Error {
    let mapped = map(e);
    let err = mapped.to_string();
    let path = path.as_str();
    debug!(
        feature = "backend",
        scheme,
        operation,
        path,
        result = "error",
        err,
        "remote op failed"
    );
    mapped
}

fn mkparent<A: RemoteAdapter>(target: &A::Target, client: &Arc<dyn RemoteClient<A::Target>>) {
    if let Some(parent) = target.parent() {
        // best-effort：父目录创建失败由随后的 write/copy 自身报错。
        let _ = mkdir_recursive::<A>(&parent, client);
    }
}

/// 递归扫描远端目录树，把所有 entry（含 Dir，与 `LocalBackend::walk` 行为对齐）收集到 `out`。
/// 单 list 失败即记 Err 不再下钻该子树；其余子树继续以"尽力而为"语义扫描。
fn walk_recursive<A: RemoteAdapter>(
    adapter: &A,
    target: &A::Target,
    out: &mut Vec<io::Result<Entry>>,
) {
    let listed = adapter
        .client()
        .list(target)
        .map_err(|e| map_and_log(A::scheme(), "list", target.path(), A::map_error, e));
    let entries = match listed {
        Ok(v) => v,
        Err(e) => {
            out.push(Err(e));
            return;
        }
    };
    for entry in entries {
        if entry.kind == EntryKind::Dir {
            match A::Target::from_location(&entry.location, adapter.ctx()) {
                Ok(sub) => walk_recursive::<A>(adapter, &sub, out),
                Err(e) => out.push(Err(e)),
            }
        }
        out.push(Ok(entry));
    }
}

/// 远端 mkdir-p：自底向上用 stat 找到第一个已存在的祖先，再自浅入深逐层 mkdir。
/// 远端协议的 mkdir 多为 POSIX 单层语义（父层缺失返回 ENOENT，如 pavao SMB），
/// 叶节点单次 mkdir 对 `{year}/{month}` 等多层 archive 模板必败。
/// `AlreadyExists` 容忍并发/重复创建；stat 的非 `NotFound` 错误（网络/权限）直接
/// 传播，避免在故障链路上盲目 mkdir。
fn mkdir_recursive<A: RemoteAdapter>(
    target: &A::Target,
    client: &Arc<dyn RemoteClient<A::Target>>,
) -> io::Result<()> {
    let mut missing: Vec<A::Target> = Vec::new();
    let mut cur = Some(target.clone());
    while let Some(t) = cur {
        // pavao/adb_client 可能把"路径不存在"包成 Other("no such file")，必须经
        // A::map_error 归一成 NotFound 才能正确驱动自底向上的祖先扫描。
        match client.stat(&t).map_err(A::map_error) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                cur = t.parent();
                missing.push(t);
            }
            Err(e) => return Err(e),
        }
    }
    for t in missing.iter().rev() {
        // 并发或重复创建：原始 ErrorKind 已是 AlreadyExists 时不映射也对；但同样
        // 防御性走一遍映射，避免 Other("File exists") 之类文案被当硬错误传播。
        if let Err(e) = client.mkdir(t).map_err(A::map_error)
            && e.kind() != io::ErrorKind::AlreadyExists
        {
            return Err(e);
        }
    }
    Ok(())
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
        if meta.size > MAX_REMOTE_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "remote text file too large: {} bytes (limit {MAX_REMOTE_TEXT_BYTES})",
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
            // client 是 Arc<dyn RemoteClient<_>>，不 impl Debug，故用 finish_non_exhaustive
            .finish_non_exhaustive()
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
            .map_err(|e| map_and_log(A::scheme(), "write", self.target.path(), A::map_error, e))
    }
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
