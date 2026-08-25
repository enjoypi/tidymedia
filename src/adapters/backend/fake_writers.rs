//! `open_read` / `open_write` 返回的注入式 IO 对象：`FailingReader` /
//! `SeekFailingReader` / `FakeWriter`。拆自 `fake.rs`（原 366 行 → ≤300），
//! 仅被 `fake.rs` 的固有 impl 与 `fake_ops.rs` 的 trait impl 引用。

use std::io::{self, Cursor, Write};

use std::sync::{Arc, Mutex};

use super::{State, file_meta};
use crate::entities::backend::MediaWriter;
use crate::entities::uri::Location;

/// 总是 read 报错的 reader。Seek 走通：只是让 `Box<dyn MediaReader>` 类型对得上。
#[derive(Debug)]
pub(super) struct FailingReader {
    pub(super) kind: io::ErrorKind,
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
pub(super) struct SeekFailingReader {
    pub(super) kind: io::ErrorKind,
    pub(super) inner: Cursor<Vec<u8>>,
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
pub(super) struct FakeWriter {
    pub(super) target: Location,
    pub(super) buffer: Vec<u8>,
    pub(super) state: Arc<Mutex<State>>,
    pub(super) write_error: Option<io::ErrorKind>,
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
