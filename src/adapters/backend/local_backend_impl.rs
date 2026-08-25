//! `impl Backend for LocalBackend`：Backend trait 各方法直接映射 `std::fs`。
//!
//! 从 `local.rs` 拆出（原 357 行 → ≤300）；路径 / rename / walk 纯助手见 `local.rs`，
//! mmap 读 + 文件写见 `local_mmap`。

use std::fs;
use std::io;

use ignore::WalkBuilder;

use super::LocalBackend;
use super::mmap::{LocalWriter, open_read_inner};
use super::{local_path, rename_or_fallback, to_metadata, walk_entry_to_io};
use crate::adapters::backend::remote::MAX_TEXT_BYTES;
use crate::entities::backend::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

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
        if len > MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("local text file too large: {len} bytes (limit {MAX_TEXT_BYTES})"),
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

    /// 双端均 Local 时声明支持原生 rename：`fs::rename` 同卷 OS 原子 +
    /// `CrossesDevices` fallback 到 `fs::copy` + `fs::remove_file`（半态 wrap Err 标记）。
    /// OS 内核负责同卷判定（`subst` / junction / mount / bind / btrfs subvol），本层
    /// 无需自实现 `dev()` 探测。
    fn supports_native_rename_to(&self, other: &dyn Backend) -> bool {
        other.scheme() == "local"
    }
}
