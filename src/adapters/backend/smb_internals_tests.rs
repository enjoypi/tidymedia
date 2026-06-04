//! `SmbBackend` 内部 helpers / 适配器测试：`map_error`、`BufferedWriter`、
//! `parent_target`、`build_target`、`SmbTarget` impl、`arc_with_client`。
//! 从 `smb_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::FakeRemoteClient;
use super::*;
use crate::entities::uri::Location;

type FakeClient = FakeRemoteClient<SmbTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::with_error_factory(|k| match k {
        io::ErrorKind::PermissionDenied => io::Error::other("smb client returned EACCES"),
        other => io::Error::from(other),
    }))
}

fn smb(path: &str) -> Location {
    Location::Smb {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>) -> SmbBackend {
    SmbBackend::with_client(client as Arc<dyn SmbClient>)
}

#[test]
fn map_smb_error_eacces_to_permission_denied() {
    let e = io::Error::other("smb client returned EACCES");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_smb_error_passthrough_other_kinds() {
    let e = io::Error::from(io::ErrorKind::TimedOut);
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn map_smb_error_passthrough_other_without_eacces() {
    let e = io::Error::other("disk full");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
    assert!(format!("{mapped}").contains("disk full"));
}

#[test]
fn smb_buffered_writer_debug_format() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn smb_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn parent_target_returns_none_for_root_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("only.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b/c.bin"),
        password: None,
        krb5_ccname: None,
    };
    let p = t.parent().unwrap();
    assert_eq!(p.path.as_str(), "a/b");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    // Utf8PathBuf::from("x.bin").parent() == Some("")，要走 if-empty 早返回
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn build_target_threads_env_password_and_krb5() {
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::set_var("SMB_PASSWORD", "secret-pw");
        std::env::set_var("KRB5CCNAME", "/tmp/krb5cc_0");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert_eq!(t.password.as_deref(), Some("secret-pw"));
    assert_eq!(t.krb5_ccname.as_deref(), Some("/tmp/krb5cc_0"));
    assert_eq!(t.user.as_deref(), Some("alice"));
    assert_eq!(t.host, "nas");
    assert_eq!(t.port, Some(445));
    assert_eq!(t.share, "photos");
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
}

#[test]
fn build_target_leaves_password_none_when_env_unset() {
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert!(t.password.is_none());
    assert!(t.krb5_ccname.is_none());
}

#[test]
fn smb_target_equality_and_debug() {
    let t1 = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x"),
        password: None,
        krb5_ccname: None,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    // 空字符串 path 让 Utf8Path::parent() 直接返 None，命中 `?` early return
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from(""),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn SmbClient> = fake_client();
    let backend = SmbBackend::arc_with_client(client);
    assert_eq!(backend.scheme(), "smb");
}

#[test]
fn smb_target_entry_location_constructs_uri() {
    let t = SmbTarget {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    let loc = t.entry_location(Utf8PathBuf::from("x/y.jpg"));
    assert_eq!(
        loc,
        Location::Smb {
            user: Some("alice".into()),
            host: "nas".into(),
            port: Some(445),
            share: "photos".into(),
            path: Utf8PathBuf::from("x/y.jpg"),
        }
    );
}

#[test]
fn smb_target_path_returns_inner_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    assert_eq!(t.path().as_str(), "a/b.jpg");
}
