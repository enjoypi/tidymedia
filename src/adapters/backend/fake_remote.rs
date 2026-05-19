//! 统一远端 Fake Client：`FakeRemoteClient<T>` 为一个 [`RemoteTarget`] 提供
//! 内存内文件存储、per-op per-path 错误注入、和 spy 机制。
//! SMB / ADB / MTP 三套测试 fake 收敛到此单一泛型，消除 ~450 行重复。

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::remote::RemoteClient;
use super::remote::RemoteTarget;
use crate::entities::backend::{Entry, EntryKind, Metadata};

/// Client 操作的错误注入键。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RemoteFakeOp {
    Stat,
    List,
    Read,
    Write,
    Unlink,
    Mkdir,
}

/// Spy：记录最近一次 client 调用看到的 target，用于协议特异断言。
pub(crate) struct Spy<T> {
    pub last_target_seen: Option<T>,
}

impl<T> Spy<T> {
    fn new() -> Self {
        Spy {
            last_target_seen: None,
        }
    }
}

/// 按文件路径存储内容 + 元数据。
struct FileEntry {
    data: Vec<u8>,
    meta: Metadata,
}

/// 统一远端 Fake Client。`error_factory` 允许 SMB 注入 EACCES 文案让
/// `map_error` 识别；ADB / MTP 直接用默认 `|k| k.into()`。
pub(crate) struct FakeRemoteClient<T: RemoteTarget> {
    files: Mutex<HashMap<Utf8PathBuf, FileEntry>>,
    op_errors: Mutex<HashMap<(RemoteFakeOp, Utf8PathBuf), io::ErrorKind>>,
    pub spy: Mutex<Spy<T>>,
    error_factory: fn(io::ErrorKind) -> io::Error,
}

impl<T: RemoteTarget> FakeRemoteClient<T> {
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            op_errors: Mutex::new(HashMap::new()),
            spy: Mutex::new(Spy::new()),
            error_factory: |k| io::Error::from(k),
        }
    }

    /// 设置自定义 error 工厂。SMB 用它把 PermissionDenied 转成含 "EACCES" 文案
    /// 的 Other error，从而触发 `map_error`。
    pub fn with_error_factory(f: fn(io::ErrorKind) -> io::Error) -> Self {
        Self {
            error_factory: f,
            ..Self::new()
        }
    }

    /// 添加文件：path 内的路径 + 数据。
    pub fn add_file(&self, path: &str, data: Vec<u8>) {
        let p = Utf8PathBuf::from(path);
        let size = data.len() as u64;
        let mut s = self.files.lock().unwrap();
        s.insert(
            p,
            FileEntry {
                data,
                meta: Metadata {
                    size,
                    kind: EntryKind::File,
                    modified: Some(SystemTime::UNIX_EPOCH),
                    created: Some(SystemTime::UNIX_EPOCH),
                },
            },
        );
    }

    /// 注入错误：下次调用 `op` 且 path 匹配时返回对应 ErrorKind。
    pub fn inject(&self, op: RemoteFakeOp, path: &str, kind: io::ErrorKind) {
        self.op_errors
            .lock()
            .unwrap()
            .insert((op, Utf8PathBuf::from(path)), kind);
    }

    fn check(&self, op: RemoteFakeOp, path: &camino::Utf8Path) -> io::Result<()> {
        let s = self.op_errors.lock().unwrap();
        if let Some(kind) = s.get(&(op, path.to_path_buf())) {
            return Err((self.error_factory)(*kind));
        }
        Ok(())
    }

    fn record(&self, target: &T) {
        self.spy.lock().unwrap().last_target_seen = Some(target.clone());
    }

    /// 测试辅助：读取已写入的文件内容。
    pub fn get_file(&self, path: &str) -> Option<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(&Utf8PathBuf::from(path))
            .map(|e| e.data.clone())
    }

    /// 测试辅助：读取已记录的文件元数据。
    pub fn get_metadata(&self, path: &str) -> Option<Metadata> {
        self.files
            .lock()
            .unwrap()
            .get(&Utf8PathBuf::from(path))
            .map(|e| e.meta.clone())
    }
}

impl<T: RemoteTarget> RemoteClient<T> for FakeRemoteClient<T> {
    fn stat(&self, target: &T) -> io::Result<Metadata> {
        self.record(target);
        self.check(RemoteFakeOp::Stat, target.path())?;
        let s = self.files.lock().unwrap();
        s.get(target.path())
            .map(|e| e.meta.clone())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, target: &T) -> io::Result<Vec<Entry>> {
        self.record(target);
        self.check(RemoteFakeOp::List, target.path())?;
        let s = self.files.lock().unwrap();
        let parent = target.path().as_str();
        Ok(s.keys()
            .filter(|p| {
                let child = p.as_str();
                parent.is_empty() || child == parent || child.starts_with(&format!("{parent}/"))
            })
            .map(|p| {
                let m = &s[p].meta;
                Entry {
                    location: target.entry_location(p.clone()),
                    size: m.size,
                    kind: m.kind,
                }
            })
            .collect())
    }

    fn read(&self, target: &T) -> io::Result<Vec<u8>> {
        self.record(target);
        self.check(RemoteFakeOp::Read, target.path())?;
        let s = self.files.lock().unwrap();
        s.get(target.path())
            .map(|e| e.data.clone())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write(&self, target: &T, data: &[u8]) -> io::Result<u64> {
        self.record(target);
        self.check(RemoteFakeOp::Write, target.path())?;
        let size = data.len() as u64;
        let mut s = self.files.lock().unwrap();
        s.insert(
            target.path().to_path_buf(),
            FileEntry {
                data: data.to_vec(),
                meta: Metadata {
                    size,
                    kind: EntryKind::File,
                    modified: Some(SystemTime::UNIX_EPOCH),
                    created: Some(SystemTime::UNIX_EPOCH),
                },
            },
        );
        Ok(size)
    }

    fn unlink(&self, target: &T) -> io::Result<()> {
        self.record(target);
        self.check(RemoteFakeOp::Unlink, target.path())?;
        let mut s = self.files.lock().unwrap();
        s.remove(target.path());
        Ok(())
    }

    fn mkdir(&self, target: &T) -> io::Result<()> {
        self.record(target);
        self.check(RemoteFakeOp::Mkdir, target.path())?;
        let mut s = self.files.lock().unwrap();
        s.insert(
            target.path().to_path_buf(),
            FileEntry {
                data: Vec::new(),
                meta: Metadata {
                    size: 0,
                    kind: EntryKind::Dir,
                    modified: None,
                    created: None,
                },
            },
        );
        Ok(())
    }
}

impl<T: RemoteTarget> std::fmt::Debug for FakeRemoteClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRemoteClient").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "fake_remote_tests.rs"]
mod tests;