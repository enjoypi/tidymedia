use std::fs;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::entities::backend::{MediaReader, MediaWriter, STREAM_BUFFER_BYTES};

#[derive(Debug)]
pub(super) struct FileReader {
    inner: BufReader<fs::File>,
}

impl FileReader {
    pub(super) fn new(file: fs::File) -> Self {
        Self {
            inner: BufReader::with_capacity(STREAM_BUFFER_BYTES, file),
        }
    }
}

impl Read for FileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for FileReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

pub(super) fn open_read_inner(path: &Path) -> io::Result<Box<dyn MediaReader>> {
    super::open_read_inner_with(path, real_open)
}

pub(super) fn real_open(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
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
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut me = *self;
        me.file.flush()
    }
}
