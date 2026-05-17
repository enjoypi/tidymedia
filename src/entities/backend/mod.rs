//! Backend Gateway 抽象：把"按 [`Location`] 做文件 IO"的差异封进单一 trait，
//! 让 entities / usecases 层不再硬绑 `std::fs`。Local/Smb/Mtp 三实现分别落在
//! 同目录的兄弟模块。CLAUDE.md「URI 与 Backend」段记录使用约定。

use std::io::{self, Read, Seek, Write};
use std::time::SystemTime;

use super::uri::Location;

pub mod local;
pub mod mtp;
pub mod smb;

#[cfg(test)]
pub(crate) mod fake;

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
/// 远程实现走客户端 read_at + 内部缓冲；两者都暴露 [`Read`] + [`Seek`]。
/// `Debug` supertrait 是为了让 `Result<Box<dyn MediaReader>, _>::unwrap_err()`
/// 等惯用模式可用——所有具体实现自行 derive 或手写。
pub trait MediaReader: Read + Seek + Send + std::fmt::Debug {}
impl<T: Read + Seek + Send + std::fmt::Debug + ?Sized> MediaReader for T {}

/// 可被 Backend 抽象返回的"按字节写"句柄。`finish` 用于远程实现在关闭
/// 句柄时执行 flush + commit；本地实现可空操作。
pub trait MediaWriter: Write + Send + std::fmt::Debug {
    fn finish(self: Box<Self>) -> io::Result<()>;
}

/// 任意存储后端的统一入口。所有 IO 入口集中到这一组方法，方便上层用
/// fake / 真实库等价替换。
pub trait Backend: Send + Sync {
    fn scheme(&self) -> &'static str;
    fn metadata(&self, loc: &Location) -> io::Result<Metadata>;
    fn exists(&self, loc: &Location) -> io::Result<bool>;
    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a>;
    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>>;
    fn open_write(
        &self,
        loc: &Location,
        mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>>;
    fn remove_file(&self, loc: &Location) -> io::Result<()>;
    fn mkdir_p(&self, loc: &Location) -> io::Result<()>;
    fn read_to_string(&self, loc: &Location) -> io::Result<String>;
    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        mkparents: bool,
    ) -> io::Result<u64>;
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
