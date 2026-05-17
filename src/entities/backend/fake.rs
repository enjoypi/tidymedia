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

use super::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
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
}

pub struct FakeBackend {
    scheme: &'static str,
    state: Arc<Mutex<State>>,
}

impl FakeBackend {
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
        s.metas.insert(
            loc,
            Metadata {
                size,
                kind: EntryKind::File,
                modified: Some(SystemTime::UNIX_EPOCH),
                created: Some(SystemTime::UNIX_EPOCH),
            },
        );
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
    /// 覆盖 "open_read 成功但 stream 阶段失败" 这类只在远端 backend 真实出现的失败模式。
    pub fn inject_reader_error(&self, loc: Location, kind: io::ErrorKind) {
        self.state.lock().unwrap().reader_errors.insert(loc, kind);
    }

    fn check_error(&self, loc: &Location, op: Op) -> io::Result<()> {
        let s = self.state.lock().unwrap();
        if let Some(kind) = s.errors.get(&(loc.clone(), op)) {
            return Err(io::Error::new(*kind, format!("injected {op:?}")));
        }
        Ok(())
    }

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
        Ok(Box::new(Cursor::new(bytes)))
    }

    fn open_write(
        &self,
        loc: &Location,
        _mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>> {
        self.check_error(loc, Op::OpenWrite)?;
        Ok(Box::new(FakeWriter {
            target: loc.clone(),
            buffer: Vec::new(),
            state: Arc::clone(&self.state),
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

    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        _mkparents: bool,
    ) -> io::Result<u64> {
        self.check_error(src, Op::CopyFile)?;
        let mut s = self.state.lock().unwrap();
        let bytes = s
            .files
            .get(src)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let size = bytes.len() as u64;
        s.files.insert(dst.clone(), bytes);
        s.metas.insert(
            dst.clone(),
            Metadata {
                size,
                kind: EntryKind::File,
                modified: Some(SystemTime::UNIX_EPOCH),
                created: Some(SystemTime::UNIX_EPOCH),
            },
        );
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

#[derive(Debug)]
struct FakeWriter {
    target: Location,
    buffer: Vec<u8>,
    state: Arc<Mutex<State>>,
}

impl Write for FakeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
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
        s.metas.insert(
            self.target,
            Metadata {
                size,
                kind: EntryKind::File,
                modified: Some(SystemTime::UNIX_EPOCH),
                created: Some(SystemTime::UNIX_EPOCH),
            },
        );
        Ok(())
    }
}

/// Location 是否位于 root 之下：scheme 必须相同；root 是 dir 等价匹配整段
/// 字符串前缀（用 [`Location::display`] 比较以避免按字段类型穷举）。
fn loc_is_under(child: &Location, root: &Location) -> bool {
    if child.scheme() != root.scheme() {
        return false;
    }
    let child_s = child.display();
    let root_s = root.display();
    child_s == root_s
        || child_s.starts_with(&format!("{root_s}/"))
}
