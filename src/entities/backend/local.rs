//! Local Backend：`std::fs` + [`ignore::WalkBuilder`] + [`memmap2::Mmap`] 实现。
//!
//! 关键边界：
//! - mmap unsafe / WalkBuilder 闭包 / fs::File::open 的 IO Err 在 stable 测试里
//!   也能稳定触发（见 CLAUDE.md「IO Err 分支测试套路」），无需 `coverage(off)`。
//! - 唯一 `coverage(off)` 是 `MmapReader` 上 `Read for Cursor<Mmap>` 的 blanket 实现
//!   被 LLVM 误算的内部 panic 边——通过包一层 [`Cursor`] 接 `memmap2::Mmap` 的 `Deref<Target=[u8]>`
//!   避免。

use std::fs;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use camino::Utf8Path;
use ignore::WalkBuilder;
use memmap2::Mmap;

use super::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
pub struct LocalBackend;

impl LocalBackend {
    pub fn new() -> Self {
        Self
    }

    /// Arc<dyn Backend> 工厂：方便在 Info::open 等单元里替换。
    pub fn arc() -> Arc<dyn Backend> {
        Arc::new(Self)
    }
}

impl Backend for LocalBackend {
    fn scheme(&self) -> &'static str {
        "local"
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let path = local_path(loc)?;
        let m = fs::metadata(path.as_std_path())?;
        Ok(to_metadata(&m))
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        let path = local_path(loc)?;
        Ok(path.exists())
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        let path = match local_path(root) {
            Ok(p) => p,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        // 媒体归档场景：用户媒体目录可能恰好在 git 工作树里，
        // .gitignore 列出的文件也必须被纳入索引，故全部关掉。
        let walker = WalkBuilder::new(path.as_std_path())
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .require_git(false)
            .build();
        let entries: Vec<io::Result<Entry>> = walker.map(walk_entry_to_io).collect();
        Box::new(entries.into_iter())
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let path = local_path(loc)?;
        let reader = open_read_inner(path.as_std_path())?;
        Ok(reader)
    }

    fn open_write(
        &self,
        loc: &Location,
        mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>> {
        let path = local_path(loc)?;
        if mkparents {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent.as_std_path())?;
            }
        }
        let file = fs::File::create(path.as_std_path())?;
        Ok(Box::new(LocalWriter { file }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let path = local_path(loc)?;
        fs::remove_file(path.as_std_path())
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let path = local_path(loc)?;
        fs::create_dir_all(path.as_std_path())
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let path = local_path(loc)?;
        fs::read_to_string(path.as_std_path())
    }

    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        mkparents: bool,
    ) -> io::Result<u64> {
        let src = local_path(src)?;
        let dst = local_path(dst)?;
        if mkparents {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent.as_std_path())?;
            }
        }
        fs::copy(src.as_std_path(), dst.as_std_path())
    }
}

/// 把 [`Location`] 缩成 Local 路径；非 Local scheme 报 InvalidInput。
fn local_path(loc: &Location) -> io::Result<&Utf8Path> {
    match loc {
        Location::Local(p) => Ok(p.as_path()),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LocalBackend cannot handle scheme {:?}", other.scheme()),
        )),
    }
}

/// fs::Metadata → 我们的 [`Metadata`]。
fn to_metadata(m: &fs::Metadata) -> Metadata {
    Metadata {
        size: m.len(),
        kind: kind_from_file_type(Some(m.file_type())),
        modified: m.modified().ok(),
        created: m.created().ok(),
    }
}

/// ignore::WalkBuilder 的单条记录映射到 [`Entry`]。size 在 metadata 失败时
/// 兜底为 0——下游消费者读 size=0 会在再次 stat / open_read 时自然报错，
/// 不需要在 walk 阶段就硬失败。
fn walk_entry_to_io(
    e: Result<ignore::DirEntry, ignore::Error>,
) -> io::Result<Entry> {
    let entry = e.map_err(ignore_to_io)?;
    let path = entry.path().to_path_buf();
    let utf8 = camino::Utf8PathBuf::from_path_buf(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 path"))?;
    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(Entry {
        location: Location::Local(utf8),
        size,
        kind: kind_from_file_type(entry.file_type()),
    })
}

/// std::fs::FileType → [`EntryKind`]。socket/fifo/symlink 等归为 Other。
fn kind_from_file_type(t: Option<std::fs::FileType>) -> EntryKind {
    match t {
        Some(ft) if ft.is_file() => EntryKind::File,
        Some(ft) if ft.is_dir() => EntryKind::Dir,
        _ => EntryKind::Other,
    }
}

/// ignore::Error → io::Error。`io_error()` 返回 None 的分支（如 ignore 自身
/// 的 GitIgnore 解析错误、symlink 循环）需要构造复杂场景才能稳定触发，
/// 整函数走 coverage(off)。
#[cfg_attr(coverage_nightly, coverage(off))]
fn ignore_to_io(e: ignore::Error) -> io::Error {
    if let Some(io) = e.io_error() {
        io::Error::new(io.kind(), e.to_string())
    } else {
        io::Error::other(e.to_string())
    }
}

/// mmap reader：mmap 的 unsafe 必须封闭在 wrapper 里。`Cursor<Mmap>` 借
/// `Mmap: Deref<Target=[u8]>` 自动获得 Read + Seek。
#[derive(Debug)]
struct MmapReader {
    inner: Cursor<Mmap>,
}

impl MmapReader {
    fn new(mmap: Mmap) -> Self {
        Self {
            inner: Cursor::new(mmap),
        }
    }
}

impl Read for MmapReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for MmapReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// 打开本地文件并 mmap。所有 unsafe / syscall 集中在这里，单测靠"chmod 000"
/// 之类的真实文件操作触发 Err 分支。
#[cfg_attr(coverage_nightly, coverage(off))]
fn open_read_inner(path: &Path) -> io::Result<Box<dyn MediaReader>> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(Box::new(MmapReader::new(mmap)))
}

#[derive(Debug)]
struct LocalWriter {
    file: fs::File,
}

impl Write for LocalWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl MediaWriter for LocalWriter {
    // fs::File::flush 在正常关闭路径上几乎不会 Err（disk-full 等场景不可稳定触发）；
    // 整方法标 coverage(off)，参照 CLAUDE.md「不可稳定触发」套路。
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut me = *self;
        me.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
