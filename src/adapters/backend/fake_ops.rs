//! [`Backend`] trait 的内存实现：全部方法统一走 `FakeBackend` 的内部 state 锁。
//! 拆自 `fake.rs`（原 366 行 → ≤300），与 `fake.rs`（类型 + 固有 impl）、
//! `fake_writers.rs`（注入式 IO 对象）平级。

use std::io::{self, Cursor};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::writers::{FailingReader, FakeWriter, SeekFailingReader};
use super::{FakeBackend, Op, file_meta};
use crate::entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
use crate::entities::common::under_prefix;
use crate::entities::uri::Location;

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
        // 计数先于 check_error：注 Err 场景也应统计「业务尝试调用」次数，让 F4 缓存
        // miss 与 Err 路径都能从 mkdir_p_calls 观察到（避免 inject_error 后 cache
        // 命中测试无法分辨「未调用」与「调用 + Err」）。
        self.mkdir_p_calls.fetch_add(1, Ordering::SeqCst);
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

/// Location 是否位于 root 之下：scheme 必须相同；root 是 dir 等价匹配整段
/// 字符串前缀。沿用 [`entities::common::under_prefix`] 的分隔符边界 + 尾分隔符
/// 剥离语义，避免与生产代码两份 prefix 检查实现漂移。
fn loc_is_under(child: &Location, root: &Location) -> bool {
    if child.scheme() != root.scheme() {
        return false;
    }
    under_prefix(&child.display(), &root.display())
}
