//! 统一远端 Fake Client：`FakeRemoteClient<T>` 为一个 [`RemoteTarget`] 提供
//! 内存内文件存储、per-op per-path 错误注入、和 spy 机制。
//! SMB / ADB / MTP 三套测试 fake 收敛到此单一泛型，消除 ~450 行重复。

use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::remote::RemoteClient;
use super::remote::RemoteTarget;
use crate::entities::backend::{Entry, EntryKind, Metadata};

/// `child` 是否为 `parent` 目录的直属项（parent/<name>，<name> 不含分隔符）。
/// 不复用 `under_prefix`：那个函数把 path == prefix 也算 true，且不剥分隔符之后的
/// 多级嵌套——对模拟 SMB/ADB list 语义需更严格的「正好一级」判定。
fn is_direct_child(child: &str, parent: &str) -> bool {
    let parent_trim = parent.strip_suffix(['/', '\\']).unwrap_or(parent);
    let Some(rest) = child.strip_prefix(parent_trim) else {
        return false;
    };
    // rest 必须以 '/' 或 '\\' 起 + 剩余 segment 中无更深分隔符
    let Some(inner) = rest.strip_prefix(['/', '\\']) else {
        return false;
    };
    !inner.is_empty() && !inner.contains('/') && !inner.contains('\\')
}

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

fn file_meta(size: u64) -> Metadata {
    Metadata {
        size,
        kind: EntryKind::File,
        modified: Some(SystemTime::UNIX_EPOCH),
        created: Some(SystemTime::UNIX_EPOCH),
    }
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

    /// 设置自定义 error 工厂。SMB 用它把 `PermissionDenied` 转成含 "EACCES" 文案
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
                meta: file_meta(size),
            },
        );
    }

    /// 添加目录：path 是 fake 命名空间内的目录路径。让多层目录树 fixture 可构造，
    /// 配合下面 `list` 的"直属子项"语义驱动 `walk_recursive` 的 Dir 递归分支。
    pub fn add_dir(&self, path: &str) {
        let p = Utf8PathBuf::from(path);
        let mut s = self.files.lock().unwrap();
        s.insert(
            p,
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
    }

    /// 注入错误：下次调用 `op` 且 path 匹配时返回对应 `ErrorKind`。
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
        // 空 parent 是 "list 全部" 的快捷（测试便利）；非空 parent 按真实 SMB/ADB
        // list 语义只返**直属子项**（child 是 parent/<name>，<name> 不含分隔符），
        // 让 `walk_recursive` 的 Dir 递归分支可在 fake 中正确驱动多层目录树而不重复。
        Ok(s.keys()
            .filter(|p| {
                let child = p.as_str();
                if parent.is_empty() {
                    return true;
                }
                is_direct_child(child, parent)
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
                meta: file_meta(size),
            },
        );
        Ok(size)
    }

    fn unlink(&self, target: &T) -> io::Result<()> {
        self.record(target);
        self.check(RemoteFakeOp::Unlink, target.path())?;
        let mut s = self.files.lock().unwrap();
        if s.remove(target.path()).is_none() {
            // 真实 SMB/ADB unlink 缺失文件返 ENOENT；fake 静默 Ok 会掩盖
            // best-effort cleanup（`let _ = remove_file(..)`）与真错 `?` 上抛
            // 之间的语义差。`error_factory` 让 SMB 测试可注入文案变体。
            return Err((self.error_factory)(io::ErrorKind::NotFound));
        }
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
