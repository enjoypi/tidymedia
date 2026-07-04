//! Backend Gateway 抽象：把"按 [`Location`] 做文件 IO"的差异封进单一 trait，
//! 让 entities / usecases 层不再硬绑 `std::fs`。具体实现（Local/SMB/MTP/ADB/Fake）
//! 落在 `adapters::backend::*`；entities 只持 trait 与值类型，编译期对 adapters
//! 零依赖（Clean Architecture 依赖方向规则）。CLAUDE.md「URI 与 Backend」段记录
//! 使用约定。

use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::time::SystemTime;

use super::uri::Location;

pub mod factory;

/// `stream_copy` 的 `BufReader` / `BufWriter` 容量：1 MiB 让远端单文件大视频的
/// `std::io::copy` 默认 8 KiB stack buffer 减少 128× syscall / RTT 次数；本地
/// buffered IO 仍受益（每 1 MiB 一次 write syscall）。CLAUDE.md 例外「算法/IO
/// 边界常量」列表内，不外置到 config。
const STREAM_BUFFER_BYTES: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub location: Location,
    pub size: u64,
    pub kind: EntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub size: u64,
    pub kind: EntryKind,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

/// 可被 Backend 抽象返回的"按字节读"句柄。本地实现包 mmap-Cursor，
/// 远程实现走客户端 `read_at` + 内部缓冲；两者都暴露 [`Read`] + [`Seek`]。
/// `Debug` supertrait 是为了让 `Result<Box<dyn MediaReader>, _>::unwrap_err()`
/// 等惯用模式可用——所有具体实现自行 derive 或手写。
pub trait MediaReader: Read + Seek + Send + std::fmt::Debug {}
impl<T: Read + Seek + Send + std::fmt::Debug + ?Sized> MediaReader for T {}

/// 可被 Backend 抽象返回的"按字节写"句柄。`finish` 用于远程实现在关闭
/// 句柄时执行 flush + commit；本地实现可空操作。
pub trait MediaWriter: Write + Send + std::fmt::Debug {
    /// 完成写入并提交（远程实现做 flush + commit，本地实现可空操作）。
    ///
    /// # Errors
    ///
    /// 当底层 flush 或网络提交失败时返回 `Err`。
    fn finish(self: Box<Self>) -> io::Result<()>;
}

/// 任意存储后端的统一入口。所有 IO 入口集中到这一组方法，方便上层用
/// fake / 真实库等价替换。
pub trait Backend: Send + Sync {
    fn scheme(&self) -> &'static str;

    /// 获取指定位置的元数据（大小、类型、修改时间等）。
    ///
    /// # Errors
    ///
    /// 当路径不存在、scheme 不匹配或底层 IO 调用失败时返回 `Err`。
    fn metadata(&self, loc: &Location) -> io::Result<Metadata>;

    /// 判断指定位置是否存在。
    ///
    /// # Errors
    ///
    /// 当 scheme 不匹配或底层 IO 调用失败时返回 `Err`。
    fn exists(&self, loc: &Location) -> io::Result<bool>;

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a>;

    /// 以只读方式打开指定位置，返回可 `Read + Seek` 的句柄。
    ///
    /// # Errors
    ///
    /// 当路径不存在、scheme 不匹配或底层打开失败时返回 `Err`。
    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>>;

    /// 以写入方式打开指定位置，返回可写句柄；`mkparents` 为 `true` 时自动创建父目录。
    ///
    /// # Errors
    ///
    /// 当 scheme 不匹配、父目录创建失败或底层打开失败时返回 `Err`。
    fn open_write(&self, loc: &Location, mkparents: bool) -> io::Result<Box<dyn MediaWriter>>;

    /// 删除指定位置的文件。
    ///
    /// # Errors
    ///
    /// 当路径不存在、scheme 不匹配或底层删除失败时返回 `Err`。
    fn remove_file(&self, loc: &Location) -> io::Result<()>;

    /// 递归创建指定位置对应的目录（含所有中间层）。
    ///
    /// # Errors
    ///
    /// 当 scheme 不匹配或底层创建失败时返回 `Err`。
    fn mkdir_p(&self, loc: &Location) -> io::Result<()>;

    /// 读取指定位置的全部内容为 UTF-8 字符串。
    ///
    /// # Errors
    ///
    /// 当路径不存在、scheme 不匹配、内容非 UTF-8 或底层 IO 失败时返回 `Err`。
    fn read_to_string(&self, loc: &Location) -> io::Result<String>;

    /// 将 `src` 文件复制到 `dst`，返回复制的字节数；`mkparents` 为 `true` 时自动创建父目录。
    ///
    /// # Errors
    ///
    /// 当 scheme 不匹配、源不存在、父目录创建失败或底层复制失败时返回 `Err`。
    fn copy_file(&self, src: &Location, dst: &Location, mkparents: bool) -> io::Result<u64>;

    /// 在同一 backend 内原子重命名/移动文件；`mkparents` 为 `true` 时自动创建目标父目录。
    ///
    /// Local 实现用 `std::fs::rename`（同一文件系统时原子，跨设备 fallback 到 copy + remove）。
    /// 远端 backend（SMB / ADB / MTP）当前吃 default 实现：`copy_file` + `remove_file`
    /// **非原子 fallback**。`SMB2` `SET_INFO` `FileRenameInformation` / ADB `shell mv` /
    /// libmtp `SendObjectPropList` Rename 均是协议原生原子 op，理想上远端 backend 应
    /// override 走原生路径（减 2 RTT + 断电原子性）；未 override 前 use case 通过
    /// [`Self::supports_native_rename_to`] 判定能力，避免误踩非原子 fallback。
    ///
    /// # Errors
    ///
    /// 当 scheme 不匹配、源不存在、父目录创建失败或底层操作失败时返回 `Err`。
    fn rename(&self, from: &Location, to: &Location, mkparents: bool) -> io::Result<()> {
        // 跨设备 fallback：先 copy 再 remove；copy 返字节数，统一丢弃。
        // copy 成功但 remove 失败的半态必须显式标记 "copied … but cannot remove
        // source"，让上层（`do_copy` / failed 计数）能与 "copy 也失败" 区分；
        // 否则用户误判后重跑会再次复制并删源致丢源。
        self.copy_file(from, to, mkparents)?;
        self.remove_file(from).map_err(|re| {
            io::Error::new(
                re.kind(),
                format!(
                    "rename fallback: copied {src} -> {dst} but cannot remove source: {re}",
                    src = from.display(),
                    dst = to.display(),
                ),
            )
        })
    }

    /// 声明本 backend 是否能与 `other` 之间做原生原子 rename（无需 copy+remove
    /// fallback）。use case 层（`copy/ops.rs::do_copy`）经此判定决定 move 走
    /// [`Self::rename`] 快路径 还是 [`stream_copy`] + `remove_file` 慢路径，避免
    /// 硬编码 `scheme() == "local"` 阻塞未来 `SMB2` / ADB / MTP 原生 rename 接入。
    ///
    /// Default 保守返 `false`，`LocalBackend` override 为 `other.scheme() == "local"`
    /// （`Local::rename` 走 `fs::rename` + `CrossesDevices` fallback，OS 内核识别
    /// `subst` / junction / mount 等所有同卷边界）。
    fn supports_native_rename_to(&self, other: &dyn Backend) -> bool {
        let _ = other;
        false
    }
}

/// 跨 backend 流式 copy 单点 helper：`copy/ops.rs::do_copy` 与
/// `cull/group_writer.rs::write_group` 共用此实现，消除双份维护成本 + 同步
/// 1 MiB `BufReader`/`BufWriter` 缓冲（cull 旧版走 stdlib 8 KiB 让远端大视频 128×
/// RTT）与失败清理逻辑。
///
/// `prefer_native_copy` = true 且同 scheme 时走 `src_be.copy_file` 让 backend 享受
/// 原生 copy（`LocalBackend` = `fs::copy` sendfile / reflink；远端未来可加 `SMB2`
/// `FSCTL_SRV_COPYCHUNK` / ADB `shell cp` 服务端复制）。**仅 caller 确认 `src_be` 与
/// `dst_be` 是同一 backend 实例时**才应传 true（用 `Arc::ptr_eq` 判定），否则同 scheme
/// 不同实例（如 Fake vs Local 均声明 `scheme=="local"`）会让 `src_be.copy_file(src, dst)`
/// 访问 `dst_be` 独立存储致 `NotFound`。false 时无论 scheme 一律走 stream 路径
/// 逐 buf 泵：`open_read` → `BufReader` → `BufWriter` → `open_write` → `io::copy`
/// → finish 三阶段闭合（`copy` 字节 / `flush` 缓冲 / `finish` 提交）确保 disk-full
/// 与远端 commit 失败不静默吞。
///
/// 中途失败清理半截 dst：`open_write` 已 create/truncate 目标，Err 时必须
/// `dst_be.remove_file(dst)` 否则占据路径槽位让下轮 unique-name 走 `_N` 后缀
/// 堆积残留。best-effort：清理失败不掩盖原始传输错误。
///
/// # Errors
///
/// - 源不可读 / 目标不可写 / `io::copy` 失败 / `writer.finish` 失败：返 `Err`
/// - 同 scheme + `prefer_native_copy` 时 `src_be.copy_file` 的 Err 直接传播（不做半截清理）
pub fn stream_copy(
    src_be: &dyn Backend,
    src: &Location,
    dst_be: &dyn Backend,
    dst: &Location,
    prefer_native_copy: bool,
) -> io::Result<u64> {
    if prefer_native_copy && src_be.scheme() == dst_be.scheme() {
        return src_be.copy_file(src, dst, false);
    }
    let reader = src_be.open_read(src)?;
    let writer = dst_be.open_write(dst, false)?;
    let mut br = BufReader::with_capacity(STREAM_BUFFER_BYTES, reader);
    let mut bw = BufWriter::with_capacity(STREAM_BUFFER_BYTES, writer);
    // 三阶段闭合：copy 字节、flush 缓冲、finish 提交。BufWriter::into_inner
    // 在 flush 失败时把 inner 一并返回让 caller 决定释放策略——这里只取 io::Error
    // 让 best-effort 清理半截目标。BufWriter Drop 会再次 flush 忽略 Err，但 inner
    // 已 move 进 finish 路径，不会重入；写失败语义一次性可观测。
    let result: io::Result<u64> = (|| {
        let bytes = io::copy(&mut br, &mut bw)?;
        bw.flush()?;
        let inner = bw.into_inner().map_err(io::IntoInnerError::into_error)?;
        inner.finish()?;
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            // best-effort：清理失败不掩盖原始传输错误（若 remove 也失败，caller 看到
            // 的仍是 stream 错误，半截 dst 靠日志 / 下轮 unique-name `_N` 后缀可察）。
            let _ = dst_be.remove_file(dst);
            Err(e)
        }
    }
}
