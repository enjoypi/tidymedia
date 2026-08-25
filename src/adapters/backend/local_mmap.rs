//! 本地 mmap 读 + 文件写：mmap unsafe 封闭在 `MmapReader`，文件写走 `LocalWriter`。
//!
//! 从 `local.rs` 拆出（原 357 行 → ≤300）。`Cursor<Mmap>` 借 `Mmap: Deref<Target=[u8]>`
//! 自动获得 Read + Seek。

use std::fs;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use memmap2::Mmap;

use crate::entities::backend::{MediaReader, MediaWriter};

/// mmap reader：mmap 的 unsafe 必须封闭在 wrapper 里。`Cursor<Mmap>` 借
/// `Mmap: Deref<Target=[u8]>` 自动获得 Read + Seek。
#[derive(Debug)]
pub(super) struct MmapReader {
    inner: Cursor<Mmap>,
}

impl MmapReader {
    pub(super) fn new(mmap: Mmap) -> Self {
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
pub(super) fn open_read_inner(path: &Path) -> io::Result<Box<dyn MediaReader>> {
    super::open_read_inner_with(path, real_mmap)
}

fn real_mmap(file: &fs::File) -> io::Result<Mmap> {
    // SAFETY: file 句柄刚由 fs::File::open 创建且仍持有；本进程不会并发 truncate
    // 该文件；外部进程修改虽可能产生未定义内容但不会破坏内存安全（memmap2 文档保证）。
    unsafe { Mmap::map(file) }
}

#[derive(Debug)]
pub(super) struct LocalWriter {
    pub(super) file: fs::File,
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
    // P0 §2：MUST 优先 ? 传播错误。std::fs::File::flush 当前是 noop，但若未来加
    // BufWriter 包装会让 disk-full 等场景静默丢数据（move 模式下源随后删除即丢失）。
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut me = *self;
        me.file.flush()
    }
}
