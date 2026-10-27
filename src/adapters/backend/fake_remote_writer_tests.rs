use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;

use super::{FileEntry, RemoteFakeOp, RemoteTarget, file_meta};
use crate::entities::backend::MediaWriter;

pub(super) struct FakeRemoteWriter<T: RemoteTarget> {
    target: T,
    files: Arc<Mutex<HashMap<Utf8PathBuf, FileEntry>>>,
    op_errors: Arc<Mutex<HashMap<(RemoteFakeOp, Utf8PathBuf), io::ErrorKind>>>,
    error_factory: fn(io::ErrorKind) -> io::Error,
    buffer: Vec<u8>,
}

impl<T: RemoteTarget> FakeRemoteWriter<T> {
    pub(super) fn new(
        target: T,
        files: Arc<Mutex<HashMap<Utf8PathBuf, FileEntry>>>,
        op_errors: Arc<Mutex<HashMap<(RemoteFakeOp, Utf8PathBuf), io::ErrorKind>>>,
        error_factory: fn(io::ErrorKind) -> io::Error,
    ) -> Self {
        Self {
            target,
            files,
            op_errors,
            error_factory,
            buffer: Vec::new(),
        }
    }
}

impl<T: RemoteTarget> std::fmt::Debug for FakeRemoteWriter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRemoteWriter")
            .field("path", &self.target.path())
            .field("buffered_bytes", &self.buffer.len())
            .finish_non_exhaustive()
    }
}

impl<T: RemoteTarget> io::Write for FakeRemoteWriter<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T: RemoteTarget> MediaWriter for FakeRemoteWriter<T> {
    fn finish(self: Box<Self>) -> io::Result<()> {
        if let Some(kind) = self
            .op_errors
            .lock()
            .unwrap()
            .get(&(RemoteFakeOp::Write, self.target.path().to_path_buf()))
        {
            return Err((self.error_factory)(*kind));
        }
        let size = self.buffer.len() as u64;
        self.files.lock().unwrap().insert(
            self.target.path().to_path_buf(),
            FileEntry {
                data: self.buffer,
                meta: file_meta(size),
            },
        );
        Ok(())
    }
}
