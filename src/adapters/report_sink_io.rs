use super::{TempPersist, write_and_persist};
use crate::entities::common;

pub(super) fn serialize<T: serde::Serialize>(report: &T) -> String {
    serde_json::to_string_pretty(report).expect("internal error: serializing report must not fail")
}

pub(super) fn try_write(path: &str, bytes: &[u8]) -> common::Result<()> {
    let parent = std::path::Path::new(path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(parent)?;
    write_and_persist(path, bytes, Box::new(tmp))
}

impl TempPersist for tempfile::NamedTempFile {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        std::io::Write::write_all(self, buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(self)
    }

    fn persist_to(self: Box<Self>, path: &str) -> std::io::Result<()> {
        (*self).persist(path).map(|_| ()).map_err(|e| e.error)
    }
}
