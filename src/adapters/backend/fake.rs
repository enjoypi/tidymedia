//! 内存版 [`crate::entities::backend::Backend`]，仅用于测试调度逻辑、错误传播与 stream hash。
//!
//! 设计要点：
//! - 内部用 [`HashMap`] 持久化"已存在的对象"（数据 + Metadata）
//! - 通过 [`FakeBackend::inject_error`] 按 (Location, [`Op`]) 注入 [`io::ErrorKind`]，
//!   覆盖现有 IO Err 分支测试套路里的 NotFound / PermissionDenied / Other
//! - `open_write` 返回的 [`writers::FakeWriter`] 在 `finish` 时把 buffer 写回 backend，
//!   测试可以验证写入后的状态

use std::collections::HashMap;

use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::entities::backend::{EntryKind, Metadata};
use crate::entities::uri::Location;

#[path = "fake_ops.rs"]
mod ops;
#[path = "fake_writers.rs"]
mod writers;

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
    /// `mkdir_p` 累计调用次数（每次 entry 都 +1，含 cache miss 与首次调用）。
    /// F4 mkdir 缓存测试用：同 `target_dir` 多次 `do_copy` 应只触发 1 次 `mkdir_p`。
    /// 原子计数避开主 state 锁，杜绝「best-effort 调用替换成 no-op」类 mutation
    /// 把 `mkdir_p` 改为 no-op 后让缓存测试静默通过。
    mkdir_p_calls: Arc<AtomicU32>,
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
            mkdir_p_calls: Arc::new(AtomicU32::new(0)),
        }
    }

    /// `mkdir_p` 累计调用次数；F4 mkdir 缓存的旁路验证点。
    #[must_use]
    pub fn mkdir_p_calls(&self) -> u32 {
        self.mkdir_p_calls.load(Ordering::SeqCst)
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

#[cfg(test)]
#[path = "fake_tests.rs"]
mod tests;
