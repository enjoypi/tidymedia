//! `remote.rs` 测试：成功路径 + `Backend` trait IO 方法的基础 ok / err 分支。
//! 共享 helpers (`DummyTarget`/`DummyClient`/`DummyAdapter`) 见 `remote_test_helpers.rs`。
//! 进阶分支（`buffered_writer` + `from_location_err` 系列 + root context）见 `remote_advanced_tests.rs`。

use std::io;
use std::sync::Arc;

use super::test_helpers::{
    DummyAdapter, DummyClient, DummyTarget, backend, backend_with_client, loc,
};
use super::*;
use crate::entities::backend::{Entry, Metadata};

// ── 成功路径 ──────────────────────────────────────────────────

#[test]
fn scheme_returns_dummy() {
    assert_eq!(backend().scheme(), "dummy");
}

#[test]
fn debug_format_includes_scheme() {
    let s = format!("{:?}", backend());
    assert!(s.contains("dummy"));
}

#[test]
fn metadata_ok() {
    let m = backend().metadata(&loc()).unwrap();
    assert_eq!(m.size, 42);
}

#[test]
fn exists_true_when_stat_ok() {
    assert!(backend().exists(&loc()).unwrap());
}

#[test]
fn exists_false_when_stat_not_found() {
    let b = backend_with_client(DummyClient::with_stat_err(io::ErrorKind::NotFound));
    assert!(!b.exists(&loc()).unwrap());
}

#[test]
fn exists_propagates_other_stat_error() {
    let b = backend_with_client(DummyClient::with_stat_err(io::ErrorKind::PermissionDenied));
    let e = b.exists(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn walk_ok() {
    let entries: Vec<_> = backend().walk(&loc()).collect();
    assert_eq!(entries.len(), 0);
}

#[test]
fn walk_list_err_propagates() {
    let b = backend_with_client(DummyClient::with_list_err(io::ErrorKind::TimedOut));
    let mut iter = b.walk(&loc());
    let e = iter.next().unwrap().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn open_read_ok() {
    let mut r = backend().open_read(&loc()).unwrap();
    let mut buf = Vec::new();
    io::Read::read_to_end(&mut r, &mut buf).unwrap();
    assert_eq!(buf, b"hello");
}

#[test]
fn open_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::ConnectionRefused));
    let e = b.open_read(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
}

#[test]
fn open_write_mkparents_false() {
    let w = backend().open_write(&loc(), false).unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn open_write_mkparents_true_ok() {
    let w = backend().open_write(&loc(), true).unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn remove_file_ok() {
    backend().remove_file(&loc()).unwrap();
}

#[test]
fn remove_file_err_propagates() {
    let c = DummyClient {
        unlink: Some(io::ErrorKind::NotFound),
        ..Default::default()
    };
    let b = backend_with_client(c);
    let e = b.remove_file(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn mkdir_p_ok() {
    backend().mkdir_p(&loc()).unwrap();
}

#[test]
fn read_to_string_ok() {
    let s = backend().read_to_string(&loc()).unwrap();
    assert_eq!(s, "hello");
}

#[test]
fn read_to_string_invalid_utf8() {
    // 用一个专门返回乱码字节的 client 触发 read_to_string 的 UTF-8 错误分支
    #[derive(Debug)]
    struct BadUtf8Client;
    impl RemoteClient<DummyTarget> for BadUtf8Client {
        // read_to_string 现在 stat-then-read：先 stat 做大小封顶后再读字节
        fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
            Ok(Metadata {
                size: 2,
                kind: crate::entities::backend::EntryKind::File,
                modified: None,
                created: None,
            })
        }
        fn list(&self, _t: &DummyTarget) -> io::Result<Vec<Entry>> {
            unreachable!()
        }
        fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
            Ok(vec![0xff, 0xfe]) // invalid UTF-8
        }
        fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
            unreachable!()
        }
        fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
        fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
    }
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(Arc::new(BadUtf8Client)),
    };
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

// 边界值：size 恰等 cap 时应被**接受**（`>` cap 检查的 inclusive 边界）。
// 杀变异：`>` → `>=` 让 size==cap 被错拒；该测试断言 size==cap 路径通畅。
#[test]
fn read_to_string_accepts_size_exactly_at_cap() {
    use crate::entities::backend::EntryKind;
    #[derive(Debug)]
    struct AtCapClient;
    impl RemoteClient<DummyTarget> for AtCapClient {
        fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
            Ok(Metadata {
                size: super::MAX_TEXT_BYTES,
                kind: EntryKind::File,
                modified: None,
                created: None,
            })
        }
        fn list(&self, _t: &DummyTarget) -> io::Result<Vec<Entry>> {
            unreachable!()
        }
        fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
            // 真实远端 read 返回的字节数不必与 stat 报告的 size 一致；上限检查仅看
            // stat 报告值，read 返回少量字节用于断言路径未被错拒。
            Ok(b"ok".to_vec())
        }
        fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
            unreachable!()
        }
        fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
        fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
    }
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(Arc::new(AtCapClient)),
    };
    let s = b.read_to_string(&loc()).unwrap();
    assert_eq!(s, "ok");
}

// 远端 sidecar 体积上限：stat 报告超过 `MAX_TEXT_BYTES` 必须直接 Err，
// 不进 client.read 一次性入堆，防不受信远端共享拖爆内存。
#[test]
fn read_to_string_rejects_file_above_size_cap() {
    use crate::entities::backend::EntryKind;
    #[derive(Debug)]
    struct HugeStatClient;
    impl RemoteClient<DummyTarget> for HugeStatClient {
        fn stat(&self, _t: &DummyTarget) -> io::Result<Metadata> {
            Ok(Metadata {
                size: super::MAX_TEXT_BYTES + 1,
                kind: EntryKind::File,
                modified: None,
                created: None,
            })
        }
        fn list(&self, _t: &DummyTarget) -> io::Result<Vec<Entry>> {
            unreachable!()
        }
        fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
            // 超限时不应到此：unreachable 既验证短路也用作 mutation kill。
            unreachable!("read must not be called when stat exceeds MAX_TEXT_BYTES")
        }
        fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
            unreachable!()
        }
        fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
        fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
    }
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(Arc::new(HugeStatClient)),
    };
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    assert!(format!("{e}").contains("too large"), "got: {e}");
}

#[test]
fn read_to_string_stat_err_propagates() {
    // stat 失败必须直接 Err，不进 read（read_to_string 现在依赖 stat 做大小封顶）。
    let b = backend_with_client(DummyClient::with_stat_err(io::ErrorKind::ConnectionRefused));
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
}

#[test]
fn read_to_string_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::TimedOut));
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn copy_file_ok_no_mkparents() {
    let n = backend().copy_file(&loc(), &loc(), false).unwrap();
    assert_eq!(n, 5);
}

#[test]
fn copy_file_ok_with_mkparents() {
    let n = backend().copy_file(&loc(), &loc(), true).unwrap();
    assert_eq!(n, 5);
}

#[test]
fn copy_file_read_err_propagates() {
    let b = backend_with_client(DummyClient::with_read_err(io::ErrorKind::ConnectionReset));
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionReset);
}
