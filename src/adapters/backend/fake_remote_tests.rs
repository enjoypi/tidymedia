//! `FakeRemoteClient` 自身的单元测试：用 `DummyTarget` 验证文件增删查改。

use super::super::remote::{RemoteClient, RemoteTarget};
use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;
use camino::{Utf8Path, Utf8PathBuf};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestTarget {
    path: Utf8PathBuf,
}

impl RemoteTarget for TestTarget {
    type Ctx = ();
    fn from_location(_loc: &Location, _ctx: &()) -> io::Result<Self> {
        Ok(TestTarget {
            path: Utf8PathBuf::from("/test"),
        })
    }
    fn parent(&self) -> Option<Self> {
        None
    }
    fn entry_location(&self, p: Utf8PathBuf) -> Location {
        Location::Local(p)
    }
    fn path(&self) -> &Utf8Path {
        &self.path
    }
}

fn client() -> FakeRemoteClient<TestTarget> {
    FakeRemoteClient::new()
}

#[test]
fn add_file_then_stat() {
    let c = client();
    c.add_file("/a.txt", b"hello".to_vec());
    let t = TestTarget {
        path: Utf8PathBuf::from("/a.txt"),
    };
    let m = c.stat(&t).unwrap();
    assert_eq!(m.size, 5);
    assert_eq!(m.kind, EntryKind::File);
}

#[test]
fn stat_not_found() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/missing"),
    };
    let e = c.stat(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn inject_error_on_stat() {
    let c = client();
    c.inject(RemoteFakeOp::Stat, "/x", io::ErrorKind::TimedOut);
    let t = TestTarget {
        path: Utf8PathBuf::from("/x"),
    };
    let e = c.stat(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn write_then_read() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/f"),
    };
    let n = c.write(&t, b"data").unwrap();
    assert_eq!(n, 4);
    let data = c.read(&t).unwrap();
    assert_eq!(data, b"data");
}

#[test]
fn unlink_removes_file() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/f"),
    };
    c.add_file("/f", b"x".to_vec());
    c.unlink(&t).unwrap();
    let e = c.stat(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn mkdir_creates_dir_entry() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/d"),
    };
    c.mkdir(&t).unwrap();
    let m = c.stat(&t).unwrap();
    assert_eq!(m.kind, EntryKind::Dir);
}

#[test]
fn list_filters_by_parent_path() {
    let c = client();
    c.add_file("/a/x.txt", b"x".to_vec());
    c.add_file("/a/y.txt", b"y".to_vec());
    c.add_file("/b/z.txt", b"z".to_vec());
    let t = TestTarget {
        path: Utf8PathBuf::from("/a"),
    };
    let entries = c.list(&t).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn spy_records_last_target() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/f"),
    };
    c.add_file("/f", b"x".to_vec());
    c.stat(&t).unwrap();
    let seen = c.spy.lock().unwrap().last_target_seen.clone().unwrap();
    assert_eq!(seen.path.as_str(), "/f");
}

#[test]
fn error_factory_transforms_kinds() {
    let c = FakeRemoteClient::<TestTarget>::with_error_factory(|k| match k {
        io::ErrorKind::PermissionDenied => io::Error::other("EACCES"),
        other => io::Error::from(other),
    });
    c.inject(RemoteFakeOp::Stat, "/x", io::ErrorKind::PermissionDenied);
    let t = TestTarget {
        path: Utf8PathBuf::from("/x"),
    };
    let e = c.stat(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::Other);
    assert!(e.to_string().contains("EACCES"));
}

#[test]
fn debug_format_non_exhaustive() {
    let c = client();
    let s = format!("{c:?}");
    assert!(s.contains("FakeRemoteClient"));
}

#[test]
fn read_returns_not_found_for_missing_key() {
    // 触发 read() 的 `s.get(...).ok_or_else(...)` None 分支：
    // 不调 add_file 且不 inject Err，让 check 通过但 map 取不到。
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/missing"),
    };
    let e = c.read(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn inject_error_on_unlink() {
    let c = client();
    c.add_file("/f", b"x".to_vec());
    c.inject(RemoteFakeOp::Unlink, "/f", io::ErrorKind::PermissionDenied);
    let t = TestTarget {
        path: Utf8PathBuf::from("/f"),
    };
    let e = c.unlink(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
}

// unlink 不存在路径返 NotFound：与真实 SMB/ADB 行为对齐；
// 覆盖 fake_remote.rs::unlink 的 is_none() True arm。
#[test]
fn unlink_returns_not_found_when_missing() {
    let c = client();
    let t = TestTarget {
        path: Utf8PathBuf::from("/never_existed"),
    };
    let e = c.unlink(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[test]
fn inject_error_on_mkdir() {
    let c = client();
    c.inject(RemoteFakeOp::Mkdir, "/d", io::ErrorKind::TimedOut);
    let t = TestTarget {
        path: Utf8PathBuf::from("/d"),
    };
    let e = c.mkdir(&t).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn list_with_empty_parent_path_includes_all_files() {
    // 触发 list filter `parent.is_empty()` 的 True 分支：parent="" 时
    // 任意 child 都应通过过滤。
    let c = client();
    c.add_file("/a.txt", b"x".to_vec());
    c.add_file("/b.txt", b"y".to_vec());
    let t = TestTarget {
        path: Utf8PathBuf::from(""),
    };
    let entries = c.list(&t).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn list_includes_entry_when_child_equals_parent() {
    // 触发 list filter `child == parent` 的 True 分支：path 自身作为 key
    // 且 path 不以末尾 `/` 形式做前缀匹配。
    let c = client();
    c.add_file("/self", b"x".to_vec());
    let t = TestTarget {
        path: Utf8PathBuf::from("/self"),
    };
    let entries = c.list(&t).unwrap();
    assert_eq!(entries.len(), 1);
}
