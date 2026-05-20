//! FakeRemoteClient 自身的单元测试：用 DummyTarget 验证文件增删查改。

use super::super::remote::{RemoteClient, RemoteTarget};
use super::*;
use crate::entities::backend::{EntryKind, Metadata};
use crate::entities::uri::Location;
use camino::{Utf8Path, Utf8PathBuf};
use std::io;
use std::sync::Arc;

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
        Location::Local(p.into())
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
    let s = format!("{:?}", c);
    assert!(s.contains("FakeRemoteClient"));
}
