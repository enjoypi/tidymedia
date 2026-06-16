//! Local Backend：`std::fs` + [`ignore::WalkBuilder`] + [`memmap2::Mmap`] 实现。
//!
//! mmap unsafe 通过 `Cursor<Mmap>` 借 `Mmap: Deref<Target=[u8]>` 收敛在 [`MmapReader::new`]。

use std::fs;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use camino::Utf8Path;
use ignore::WalkBuilder;
use memmap2::Mmap;

use crate::entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
pub struct LocalBackend;

impl LocalBackend {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Arc<dyn Backend> 工厂：方便在 `Info::open` 等单元里替换。
    #[must_use]
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
        // 必须用 try_exists：path.exists() 把 PermissionDenied 等 IO 错误吞成 false，
        // 让 naming::generate_unique_name 误判目标不存在 → open_write 覆盖现有文件，
        // move 模式下源随后被删即永久数据丢失（CLAUDE.md「Gotcha」R3 守门）。
        path.as_std_path().try_exists()
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
        Box::new(walker.map(walk_entry_to_io))
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let path = local_path(loc)?;
        let reader = open_read_inner(path.as_std_path())?;
        Ok(reader)
    }

    fn open_write(&self, loc: &Location, mkparents: bool) -> io::Result<Box<dyn MediaWriter>> {
        let path = local_path(loc)?;
        if mkparents && let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
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
        // sidecar.rs 唯一消费者：XMP/Takeout JSON 实测 < 10 KiB。不受信媒体目录
        // （USB/SD/网盘挂载）下注入 1 GB 假 `.xmp` 会让 read_to_string 一次性入堆
        // 致 OOM；与 RemoteBackend 共享同口径 MAX_TEXT_BYTES 上限。
        let len = fs::metadata(path.as_std_path())?.len();
        if len > super::remote::MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "local text file too large: {} bytes (limit {})",
                    len,
                    super::remote::MAX_TEXT_BYTES,
                ),
            ));
        }
        fs::read_to_string(path.as_std_path())
    }

    fn copy_file(&self, src: &Location, dst: &Location, mkparents: bool) -> io::Result<u64> {
        let src = local_path(src)?;
        let dst = local_path(dst)?;
        if mkparents && let Some(parent) = dst.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }
        fs::copy(src.as_std_path(), dst.as_std_path())
    }

    /// `std::fs::rename` 在同一文件系统时是原子操作。跨设备（`ErrorKind::CrossesDevices`）
    /// 时 std 返回 Err，fallback 走 trait default 的 `copy_file` + `remove_file` 两步。
    fn rename(&self, from: &Location, to: &Location, mkparents: bool) -> io::Result<()> {
        let from_path = local_path(from)?;
        let to_path = local_path(to)?;
        if mkparents && let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }
        rename_or_fallback(from_path.as_std_path(), to_path.as_std_path())
    }
}

/// 把 [`Location`] 缩成 Local 路径；非 Local scheme 报 `InvalidInput`。
fn local_path(loc: &Location) -> io::Result<&Utf8Path> {
    match loc {
        Location::Local(p) => Ok(p.as_path()),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LocalBackend cannot handle scheme {:?}", other.scheme()),
        )),
    }
}

/// 尝试 `fs::rename`；跨设备（`CrossesDevices`）时 fallback 到 copy + remove 两步。
/// 语义由 `rename_same_dir_moves_file_atomically` 等断言不退化。
fn rename_or_fallback(from: &std::path::Path, to: &std::path::Path) -> io::Result<()> {
    rename_or_fallback_with(from, to, real_rename, real_copy, real_remove)
}

pub(super) fn real_rename(a: &std::path::Path, b: &std::path::Path) -> io::Result<()> {
    fs::rename(a, b)
}
pub(super) fn real_copy(a: &std::path::Path, b: &std::path::Path) -> io::Result<u64> {
    fs::copy(a, b)
}
pub(super) fn real_remove(a: &std::path::Path) -> io::Result<()> {
    fs::remove_file(a)
}

/// 参数化版本：让单测可注入 mock rename 返 `CrossesDevices` 触发 fallback
/// （Linux 容器内 cross-mount tmpfs 需 root 不可在 ecs-user 触发）。
/// 跨设备 fallback 是 copy + remove 两步，非原子；copy 成功但 remove 失败时，
/// 文件存在于 src 与 dst 两处，Err 包裹该半态以便上层（`do_copy` / failed 计数）
/// 与 copy 也失败的场景区分，避免用户误判后再次执行致丢源。
pub(super) fn rename_or_fallback_with(
    from: &std::path::Path,
    to: &std::path::Path,
    rename: fn(&std::path::Path, &std::path::Path) -> io::Result<()>,
    copy: fn(&std::path::Path, &std::path::Path) -> io::Result<u64>,
    remove: fn(&std::path::Path) -> io::Result<()>,
) -> io::Result<()> {
    match rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy(from, to)?;
            remove(from).map_err(|re| {
                io::Error::new(
                    re.kind(),
                    format!(
                        "cross-device rename: copied {} -> {} but cannot remove source: {re}",
                        from.display(),
                        to.display()
                    ),
                )
            })
        }
        Err(e) => Err(e),
    }
}

/// `fs::Metadata` → 我们的 [`Metadata`]。
fn to_metadata(m: &fs::Metadata) -> Metadata {
    Metadata {
        size: m.len(),
        kind: kind_from_file_type(Some(m.file_type())),
        modified: m.modified().ok(),
        created: m.created().ok(),
    }
}

/// `ignore::WalkBuilder` 的单条记录映射到 [`Entry`]。metadata 失败时返回 Err 让
/// `visit_location` 计入 `walker_errors`——曾用 `map_or(0, ...)` 把 size 兜底成 0
/// 会让该 entry 落入 `skipped_empty` 路径，与真正 0 字节文件混淆，运维诊断时
/// `skipped_empty` 虚高、`walker_errors` 漏报。
fn walk_entry_to_io(e: Result<ignore::DirEntry, ignore::Error>) -> io::Result<Entry> {
    walk_entry_to_io_with(e, real_dir_entry_metadata)
}

pub(super) fn real_dir_entry_metadata(
    entry: &ignore::DirEntry,
) -> Result<std::fs::Metadata, ignore::Error> {
    entry.metadata()
}

/// 参数化版本：让单测可注入 mock `get_meta` 返 Err 触发"metadata failed" `?` 路径
/// （`ignore::DirEntry` 的 metadata 仅在文件被并发删除等罕见情况下失败，CI 不可稳定真触发）。
pub(super) fn walk_entry_to_io_with(
    e: Result<ignore::DirEntry, ignore::Error>,
    get_meta: fn(&ignore::DirEntry) -> Result<std::fs::Metadata, ignore::Error>,
) -> io::Result<Entry> {
    let entry = e.map_err(|e| ignore_to_io(&e))?;
    let path = entry.path().to_path_buf();
    let utf8 = camino::Utf8PathBuf::from_path_buf(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 path"))?;
    let size = get_meta(&entry)
        .map_err(|e| io::Error::other(format!("metadata failed for {utf8}: {e}")))?
        .len();
    Ok(Entry {
        location: Location::Local(utf8),
        size,
        kind: kind_from_file_type(entry.file_type()),
    })
}

/// `std::fs::FileType` → [`EntryKind`]。socket/fifo/symlink 等归为 Other。
fn kind_from_file_type(t: Option<std::fs::FileType>) -> EntryKind {
    match t {
        Some(ft) if ft.is_file() => EntryKind::File,
        Some(ft) if ft.is_dir() => EntryKind::Dir,
        _ => EntryKind::Other,
    }
}

/// `ignore::Error` → `io::Error`。`io_error()` None 的分支（GitIgnore 解析错误、symlink 循环）
/// 在 stable test 里不可稳定触发。
fn ignore_to_io(e: &ignore::Error) -> io::Error {
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
fn open_read_inner(path: &Path) -> io::Result<Box<dyn MediaReader>> {
    open_read_inner_with(path, real_mmap)
}

fn real_mmap(file: &fs::File) -> io::Result<Mmap> {
    // SAFETY: file 句柄刚由 fs::File::open 创建且仍持有；本进程不会并发 truncate
    // 该文件；外部进程修改虽可能产生未定义内容但不会破坏内存安全（memmap2 文档保证）。
    unsafe { Mmap::map(file) }
}

/// 参数化版本：让单测可注入 mock `mmap_fn` 返 Err 触发 `?` Err arm
/// （memmap2 在 Linux 上对 0 字节文件不返 Err，无法稳定真触发）。
pub(super) fn open_read_inner_with(
    path: &Path,
    mmap_fn: fn(&fs::File) -> io::Result<Mmap>,
) -> io::Result<Box<dyn MediaReader>> {
    let file = fs::File::open(path)?;
    let mmap = mmap_fn(&file)?;
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
    // fs::File::flush 对未 BufWriter 包装的 std::fs::File 是 noop；这里 best-effort
    // 调用并忽略可能的 Err（disk-full 等场景测试不可稳定触发）。
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut me = *self;
        me.file.flush().ok();
        Ok(())
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "local_rename_tests.rs"]
mod rename_tests;

#[cfg(test)]
#[path = "local_edge_tests.rs"]
mod edge_tests;
