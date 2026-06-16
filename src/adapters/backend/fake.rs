//! 内存版 [`Backend`]，仅用于测试调度逻辑、错误传播与 stream hash。
//!
//! 设计要点：
//! - 内部用 [`HashMap`] 持久化"已存在的对象"（数据 + Metadata）
//! - 通过 [`FakeBackend::inject_error`] 按 (Location, [`Op`]) 注入 [`io::ErrorKind`]，
//!   覆盖现有 IO Err 分支测试套路里的 NotFound / PermissionDenied / Other
//! - `open_write` 返回的 [`FakeWriter`] 在 `finish` 时把 buffer 写回 backend，
//!   测试可以验证写入后的状态

use std::collections::HashMap;

use std::io::{self, Cursor, Write};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
use crate::entities::common::under_prefix;
use crate::entities::uri::Location;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Metadata,
    Exists,
    Walk,
    OpenRead,
    OpenWrite,
    RemoveFile,
    MkdirP,
    ReadToString,
    CopyFile,
}

#[derive(Debug, Default)]
struct State {
    files: HashMap<Location, Vec<u8>>,
    metas: HashMap<Location, Metadata>,
    errors: HashMap<(Location, Op), io::ErrorKind>,
    /// 让 `open_read` 成功但返回的 reader 在 read 时立即报错；用于覆盖
    /// 调用方在 stream hash / 解析阶段的 `?` Err 分支。
    reader_errors: HashMap<Location, io::ErrorKind>,
    /// 让 `open_read` 成功但返回的 reader 在 seek 时立即报错；read 仍透传
    /// 内容。覆盖 `sniff_mime` 等 "read OK 后 seek 失败" 的 Err 分支
    /// （Cursor seek 永不失败，CLAUDE.md「Cursor 的 seek 永不失败」套路）。
    seek_errors: HashMap<Location, io::ErrorKind>,
    /// 让 `open_write` 成功但返回的 writer 在 `write` 时立即报错。覆盖
    /// `stream_copy` 中 `std::io::copy` 阶段失败（区别于 `Op::OpenWrite` 早返）。
    writer_errors: HashMap<Location, io::ErrorKind>,
}

pub struct FakeBackend {
    scheme: &'static str,
    state: Arc<Mutex<State>>,
}

fn file_meta(size: u64) -> Metadata {
    Metadata {
        size,
        kind: EntryKind::File,
        modified: Some(SystemTime::UNIX_EPOCH),
        created: Some(SystemTime::UNIX_EPOCH),
    }
}

impl FakeBackend {
    #[must_use]
    pub fn new(scheme: &'static str) -> Self {
        Self {
            scheme,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn add_file(&self, loc: Location, data: Vec<u8>) {
        let mut s = self.state.lock().unwrap();
        let size = data.len() as u64;
        s.files.insert(loc.clone(), data);
        s.metas.insert(loc, file_meta(size));
    }

    /// 覆写默认 EPOCH 时间元数据：构造 `modified`/`created` 任意组合（如
    /// `modified=None`），供 `create_time` 边界测试——真实文件系统造不出 mtime 缺失。
    pub fn add_file_with_times(
        &self,
        loc: &Location,
        data: Vec<u8>,
        modified: Option<std::time::SystemTime>,
        created: Option<std::time::SystemTime>,
    ) {
        self.add_file(loc.clone(), data);
        let mut s = self.state.lock().unwrap();
        let meta = s
            .metas
            .get_mut(loc)
            .expect("internal: add_file just inserted this meta");
        meta.modified = modified;
        meta.created = created;
    }

    pub fn add_dir(&self, loc: Location) {
        let mut s = self.state.lock().unwrap();
        s.metas.insert(
            loc,
            Metadata {
                size: 0,
                kind: EntryKind::Dir,
                modified: None,
                created: None,
            },
        );
    }

    pub fn inject_error(&self, loc: Location, op: Op, kind: io::ErrorKind) {
        self.state.lock().unwrap().errors.insert((loc, op), kind);
    }

    /// 让针对 `loc` 的 `open_read` 返回一个 reader：调用 `read` 时立即抛 `kind`。
    /// 覆盖 "`open_read` 成功但 stream 阶段失败" 这类只在远端 backend 真实出现的失败模式。
    pub fn inject_reader_error(&self, loc: Location, kind: io::ErrorKind) {
        self.state.lock().unwrap().reader_errors.insert(loc, kind);
    }

    /// 让针对 `loc` 的 reader 在 `seek` 时立即抛 `kind`；read 透传内容。
    /// Cursor seek 永不失败，需该 helper 覆盖 `sniff_mime` 等的 seek `?` Err arm。
    pub fn inject_seek_error(&self, loc: Location, kind: io::ErrorKind) {
        self.state.lock().unwrap().seek_errors.insert(loc, kind);
    }

    /// 让针对 `loc` 的 `open_write` 返回的 writer 在 `write` 时立即抛 `kind`。
    /// 覆盖 `stream_copy` 内 `std::io::copy` 写阶段失败的 Err arm，区别于
    /// `inject_error(loc, Op::OpenWrite, ..)` 让 `open_write` 自身早返。
    pub fn inject_writer_error(&self, loc: Location, kind: io::ErrorKind) {
        self.state.lock().unwrap().writer_errors.insert(loc, kind);
    }

    fn check_error(&self, loc: &Location, op: Op) -> io::Result<()> {
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.errors.get(&(loc.clone(), op)) {
            return Err(io::Error::new(*kind, format!("injected {op:?}")));
        }
        Ok(())
    }

    #[must_use]
    pub fn read_bytes(&self, loc: &Location) -> Option<Vec<u8>> {
        self.state.lock().unwrap().files.get(loc).cloned()
    }
}

impl Backend for FakeBackend {
    fn scheme(&self) -> &'static str {
        self.scheme
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        self.check_error(loc, Op::Metadata)?;
        let s = self.state.lock().unwrap();
        s.metas
            .get(loc)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        self.check_error(loc, Op::Exists)?;
        Ok(self.state.lock().unwrap().metas.contains_key(loc))
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        if let Err(e) = self.check_error(root, Op::Walk) {
            return Box::new(std::iter::once(Err(e)));
        }
        let s = self.state.lock().unwrap();
        let entries: Vec<io::Result<Entry>> = s
            .metas
            .iter()
            .filter(|(k, _)| loc_is_under(k, root))
            .map(|(k, m)| {
                Ok(Entry {
                    location: k.clone(),
                    size: m.size,
                    kind: m.kind,
                })
            })
            .collect();
        Box::new(entries.into_iter())
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        self.check_error(loc, Op::OpenRead)?;
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.reader_errors.get(loc) {
            return Ok(Box::new(FailingReader { kind: *kind }));
        }
        let bytes = s
            .files
            .get(loc)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        if let Some(kind) = s.seek_errors.get(loc) {
            return Ok(Box::new(SeekFailingReader {
                kind: *kind,
                inner: Cursor::new(bytes),
            }));
        }
        Ok(Box::new(Cursor::new(bytes)))
    }

    fn open_write(&self, loc: &Location, _mkparents: bool) -> io::Result<Box<dyn MediaWriter>> {
        self.check_error(loc, Op::OpenWrite)?;
        let write_error = self.state.lock().unwrap().writer_errors.get(loc).copied();
        Ok(Box::new(FakeWriter {
            target: loc.clone(),
            buffer: Vec::new(),
            state: Arc::clone(&self.state),
            write_error,
        }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        self.check_error(loc, Op::RemoveFile)?;
        let mut s = self.state.lock().unwrap();
        if s.metas.remove(loc).is_none() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        s.files.remove(loc);
        Ok(())
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        self.check_error(loc, Op::MkdirP)?;
        let mut s = self.state.lock().unwrap();
        s.metas.entry(loc.clone()).or_insert(Metadata {
            size: 0,
            kind: EntryKind::Dir,
            modified: None,
            created: None,
        });
        Ok(())
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        self.check_error(loc, Op::ReadToString)?;
        let s = self.state.lock().unwrap();
        let bytes = s
            .files
            .get(loc)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        String::from_utf8(bytes.clone()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(&self, src: &Location, dst: &Location, _mkparents: bool) -> io::Result<u64> {
        self.check_error(src, Op::CopyFile)?;
        let mut s = self.state.lock().unwrap();
        let bytes = s
            .files
            .get(src)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let size = bytes.len() as u64;
        s.files.insert(dst.clone(), bytes);
        s.metas.insert(dst.clone(), file_meta(size));
        Ok(size)
    }
}

/// 总是 read 报错的 reader。Seek 走通：只是让 `Box<dyn MediaReader>` 类型对得上。
#[derive(Debug)]
struct FailingReader {
    kind: io::ErrorKind,
}

impl io::Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(self.kind))
    }
}

impl io::Seek for FailingReader {
    fn seek(&mut self, _: io::SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

/// read 透传 Cursor 内容；seek 立即按 `kind` 报错。
#[derive(Debug)]
struct SeekFailingReader {
    kind: io::ErrorKind,
    inner: Cursor<Vec<u8>>,
}

impl io::Read for SeekFailingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl io::Seek for SeekFailingReader {
    fn seek(&mut self, _: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::from(self.kind))
    }
}

#[derive(Debug)]
struct FakeWriter {
    target: Location,
    buffer: Vec<u8>,
    state: Arc<Mutex<State>>,
    write_error: Option<io::ErrorKind>,
}

impl Write for FakeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(kind) = self.write_error {
            return Err(io::Error::new(kind, "injected FakeWriter::write"));
        }
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MediaWriter for FakeWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut s = self.state.lock().unwrap();
        let size = self.buffer.len() as u64;
        s.files.insert(self.target.clone(), self.buffer);
        s.metas.insert(self.target, file_meta(size));
        Ok(())
    }
}

/// Location 是否位于 root 之下：scheme 必须相同；root 是 dir 等价匹配整段
/// 字符串前缀。沿用 [`entities::common::under_prefix`] 的分隔符边界 + 尾分隔符
/// 剥离语义，避免与生产代码两份 prefix 检查实现漂移。
fn loc_is_under(child: &Location, root: &Location) -> bool {
    if child.scheme() != root.scheme() {
        return false;
    }
    under_prefix(&child.display(), &root.display())
}

#[cfg(test)]
mod tests {
    use std::io;

    use camino::Utf8PathBuf;

    use super::{FakeBackend, Op};
    use crate::entities::backend::Backend;
    use crate::entities::uri::Location;

    fn smb_loc(path: &str) -> Location {
        Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "photos".into(),
            path: Utf8PathBuf::from(path),
        }
    }

    // rename default impl：copy_file + remove_file 两步，src 消失 dst 出现。
    #[test]
    fn rename_default_moves_file_via_copy_and_remove() {
        let b = FakeBackend::new("smb");
        let src = smb_loc("a.bin");
        let dst = smb_loc("b.bin");
        b.add_file(src.clone(), b"hello".to_vec());
        b.rename(&src, &dst, false).unwrap();
        assert!(b.read_bytes(&src).is_none(), "src must be removed");
        assert_eq!(b.read_bytes(&dst).as_deref(), Some(b"hello".as_ref()));
    }

    // copy_file 失败时 rename 传播错误，remove_file 不被调用（copy 提前返 Err）。
    #[test]
    fn rename_propagates_copy_error() {
        let b = FakeBackend::new("smb");
        let src = smb_loc("a.bin");
        let dst = smb_loc("b.bin");
        b.add_file(src.clone(), b"x".to_vec());
        b.inject_error(src.clone(), Op::CopyFile, io::ErrorKind::PermissionDenied);
        let err = b.rename(&src, &dst, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // src 依然存在（copy 失败，remove 未执行）
        assert!(
            b.read_bytes(&src).is_some(),
            "src must remain on copy failure"
        );
    }

    // remove_file 失败时 rename 传播错误（copy 已成功，只有 remove 失败）。
    #[test]
    fn rename_propagates_remove_error() {
        let b = FakeBackend::new("smb");
        let src = smb_loc("a.bin");
        let dst = smb_loc("b.bin");
        b.add_file(src.clone(), b"x".to_vec());
        b.inject_error(src.clone(), Op::RemoveFile, io::ErrorKind::PermissionDenied);
        let err = b.rename(&src, &dst, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
