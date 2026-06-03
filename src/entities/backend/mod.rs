//! Backend Gateway 抽象：把"按 [`Location`] 做文件 IO"的差异封进单一 trait，
//! 让 entities / usecases 层不再硬绑 `std::fs`。Local/Smb/Mtp 三实现分别落在
//! 同目录的兄弟模块。CLAUDE.md「URI 与 Backend」段记录使用约定。

use std::io::{self, Read, Seek, Write};
use std::time::SystemTime;

use super::uri::Location;

pub mod adb {
    pub use crate::adapters::backend::adb::*;
}
pub mod local {
    pub use crate::adapters::backend::local::LocalBackend;
}
pub mod mtp {
    pub use crate::adapters::backend::mtp::*;
}
pub mod smb {
    pub use crate::adapters::backend::smb::*;
}

// FakeBackend 是常驻编译的测试 helper：集成测试（`tests/`）需要在 `#[cfg(test)]`
// 之外引用它来组装 FakeBackendFactory。`#[doc(hidden)]` 让它不出现在公开 docs。
#[doc(hidden)]
pub mod fake {
    pub use crate::adapters::backend::fake::*;
}

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
}
