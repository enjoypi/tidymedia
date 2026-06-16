//! Backend Gateway 抽象：把"按 [`Location`] 做文件 IO"的差异封进单一 trait，
//! 让 entities / usecases 层不再硬绑 `std::fs`。具体实现（Local/SMB/MTP/ADB/Fake）
//! 落在 `adapters::backend::*`；entities 只持 trait 与值类型，编译期对 adapters
//! 零依赖（Clean Architecture 依赖方向规则）。CLAUDE.md「URI 与 Backend」段记录
//! 使用约定。

use std::io::{self, Read, Seek, Write};
use std::time::SystemTime;

use super::uri::Location;

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
    /// 远端 backend（SMB / ADB / MTP）吃 default 实现：`copy_file` + `remove_file`（非原子 fallback）。
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
}
