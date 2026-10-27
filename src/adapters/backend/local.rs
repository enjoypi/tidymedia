//!
//! 拆分为三文件（原 357 行 → ≤300）：`local_backend_impl`（`impl Backend for LocalBackend`）、
//! 路径 / rename / walk 纯助手 + 测试装配）。

#[path = "local_backend_impl.rs"]
mod backend_impl;
#[path = "local_io.rs"]
mod local_io;
#[path = "local_noreplace.rs"]
mod noreplace;

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use crate::entities::backend::{
    Backend, Entry, EntryKind, MediaReader, Metadata, partial_move_error,
};
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

/// 把 [`Location`] 缩成 Local 路径；非 Local scheme 报 `InvalidInput`。
fn local_path(loc: &Location) -> io::Result<&Utf8Path> {
    match loc {
        Location::Local(p) => Ok(p.as_path()),
        other @ Location::Smb { .. } => Err(io::Error::new(
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
                partial_move_error(
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

pub(super) fn walk_entry_to_io_with(
    e: Result<ignore::DirEntry, ignore::Error>,
    get_meta: fn(&ignore::DirEntry) -> Result<std::fs::Metadata, ignore::Error>,
) -> io::Result<Entry> {
    walk_entry_to_io_convert(e, get_meta, utf8_path_buf)
}

pub(super) fn walk_entry_to_io_convert(
    e: Result<ignore::DirEntry, ignore::Error>,
    get_meta: fn(&ignore::DirEntry) -> Result<std::fs::Metadata, ignore::Error>,
    convert_path: fn(std::path::PathBuf) -> io::Result<Utf8PathBuf>,
) -> io::Result<Entry> {
    let entry = e.map_err(|e| ignore_to_io(&e))?;
    let utf8 = convert_path(entry.path().to_path_buf())?;
    let size = get_meta(&entry)
        .map_err(|e| io::Error::other(format!("metadata failed for {utf8}: {e}")))?
        .len();
    Ok(Entry {
        location: Location::Local(utf8),
        size,
        kind: kind_from_file_type(entry.file_type()),
    })
}

fn utf8_path_buf(path: std::path::PathBuf) -> io::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path).map_err(non_utf8_path_err)
}

/// 非 UTF-8 路径 → `InvalidData`。具名 fn 替代 `map_err` 内联 closure：macOS APFS
/// 强制文件名 UTF-8 无法造真实 fixture（EILSEQ），closure 在 macOS 物理不可达成
/// function miss；具名 fn 可用内存构造的 `PathBuf`（无需写盘）直测，全平台可覆盖。
pub(super) fn non_utf8_path_err(_: std::path::PathBuf) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 path")
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

pub(super) fn open_read_inner_with(
    path: &Path,
    open_fn: fn(&Path) -> io::Result<fs::File>,
) -> io::Result<Box<dyn MediaReader>> {
    let file = open_fn(path)?;
    Ok(Box::new(local_io::FileReader::new(file)))
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

#[cfg(test)]
#[path = "local_noreplace_tests.rs"]
mod noreplace_tests;
