//! `MtpTarget` / `MtpMatch` 类型语义 + writer trait + rename / `read_to_string` /
//! `copy_file` 行为测试（从 `mtp_tests.rs` 拆出）。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use super::*;
use crate::entities::uri::Location;

// MTP 测试共用 FakeRemoteClient<MtpTarget>，默认 error_factory（不需文案注入）。
type FakeClient = FakeRemoteClient<MtpTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::new())
}

fn mtp(path: &str) -> Location {
    Location::Mtp {
        device: "Pixel 8".into(),
        storage: "Internal shared storage".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>, dm: MtpMatch, sm: MtpMatch) -> MtpBackend {
    MtpBackend::with_client(client as Arc<dyn MtpClient>, dm, sm)
}

fn fuzzy_backend(client: Arc<FakeClient>) -> MtpBackend {
    backend_with(client, MtpMatch::Fuzzy, MtpMatch::Fuzzy)
}
#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("a/b/c.jpg"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    let p = t.parent().unwrap();
    assert_eq!(p.path.as_str(), "a/b");
    assert_eq!(p.device, "d");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("x.jpg"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from(""),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_none_when_parent_is_root_slash() {
    // Utf8PathBuf::from("/foo").parent() == Some("/")：与 AdbTarget::parent 对齐，
    // 哨兵让 mkdir_recursive 不会对 storage 根发出 mkdir('/') 调用。
    let t = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("/foo"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Fuzzy,
    };
    assert!(t.parent().is_none(), "/foo 的 parent '/' 被哨兵拒返 None");
}

#[test]
fn mtp_target_equality_and_debug() {
    let t1 = MtpTarget {
        device: "d".into(),
        storage: "s".into(),
        path: Utf8PathBuf::from("x"),
        device_match: MtpMatch::Exact,
        storage_match: MtpMatch::Fuzzy,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn mtp_match_distinct_variants_and_hashable() {
    use std::collections::HashSet;
    let mut s = HashSet::new();
    s.insert(MtpMatch::Exact);
    s.insert(MtpMatch::Fuzzy);
    s.insert(MtpMatch::Exact);
    assert_eq!(s.len(), 2);
    let _ = format!("{:?}", MtpMatch::Exact);
}

#[test]
fn mtp_buffered_writer_debug_format() {
    use std::io::Write;
    let client = fake_client();
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"), "got: {s}");
    assert!(s.contains("buffered_bytes"));
}

#[test]
fn mtp_buffered_writer_flush_ok() {
    use std::io::Write;
    let client = fake_client();
    let backend = fuzzy_backend(client);
    let mut w = backend.open_write(&mtp("x.jpg"), false).unwrap();
    assert!(w.flush().is_ok());
    w.finish().unwrap();
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn MtpClient> = fake_client();
    let backend = MtpBackend::arc_with_client(client, MtpMatch::Fuzzy, MtpMatch::Exact);
    assert_eq!(backend.scheme(), "mtp");
}

#[test]
fn mtp_target_entry_location_constructs_uri() {
    let t = MtpTarget {
        device: "Pixel 8".into(),
        storage: "Internal".into(),
        path: Utf8PathBuf::from("DCIM/a.jpg"),
        device_match: MtpMatch::Fuzzy,
        storage_match: MtpMatch::Exact,
    };
    let loc = t.entry_location(Utf8PathBuf::from("DCIM/b.jpg"));
    assert_eq!(
        loc,
        Location::Mtp {
            device: "Pixel 8".into(),
            storage: "Internal".into(),
            path: Utf8PathBuf::from("DCIM/b.jpg"),
        }
    );
}

#[test]
fn mtp_target_path_returns_inner_path() {
    let t = MtpTarget {
        device: "Pixel".into(),
        storage: "S".into(),
        path: Utf8PathBuf::from("DCIM/photo.jpg"),
        device_match: MtpMatch::Exact,
        storage_match: MtpMatch::Exact,
    };
    assert_eq!(t.path().as_str(), "DCIM/photo.jpg");
}

#[test]
fn rename_default_moves_file_via_copy_remove() {
    let client = fake_client();
    client.add_file("DCIM/a.jpg", b"photo".to_vec());
    let backend = fuzzy_backend(client.clone());
    backend
        .rename(&mtp("DCIM/a.jpg"), &mtp("Inbox/a.jpg"), false)
        .unwrap();
    assert!(client.get_file("DCIM/a.jpg").is_none(), "src must be gone");
    assert_eq!(
        client.get_file("Inbox/a.jpg").as_deref(),
        Some(b"photo".as_ref())
    );
}

#[test]
fn rename_propagates_copy_error_and_leaves_src() {
    let client = fake_client();
    client.add_file("src.bin", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "src.bin", io::ErrorKind::TimedOut);
    let backend = fuzzy_backend(client.clone());
    let err = backend
        .rename(&mtp("src.bin"), &mtp("dst.bin"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    // copy 失败，src 应仍存在
    assert!(client.get_file("src.bin").is_some(), "src must remain");
}

#[test]
fn rename_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.rename(&local, &mtp("dst.bin"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_decodes_utf8() {
    let client = fake_client();
    client.add_file("a.txt", b"hello-mtp".to_vec());
    let backend = fuzzy_backend(client);
    let s = backend.read_to_string(&mtp("a.txt")).unwrap();
    assert_eq!(s, "hello-mtp");
}

#[test]
fn read_to_string_rejects_invalid_utf8() {
    let client = fake_client();
    client.add_file("a.txt", vec![0xFF, 0xFE]);
    let backend = fuzzy_backend(client);
    let err = backend.read_to_string(&mtp("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_to_string_rejects_non_mtp_scheme() {
    let backend = fuzzy_backend(fake_client());
    let err = backend
        .read_to_string(&Location::Local(Utf8PathBuf::from("/tmp/x")))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_propagates_client_error() {
    let client = fake_client();
    client.add_file("a.txt", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "a.txt", io::ErrorKind::ConnectionReset);
    let backend = fuzzy_backend(client);
    let err = backend.read_to_string(&mtp("a.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

#[test]
fn copy_file_reads_then_writes_with_mkparent() {
    let client = fake_client();
    client.add_file("src.jpg", b"abc".to_vec());
    let backend = fuzzy_backend(client.clone());
    let bytes = backend
        .copy_file(&mtp("src.jpg"), &mtp("DCIM/dst.jpg"), true)
        .unwrap();
    assert_eq!(bytes, 3);
    let stored = client.get_file("DCIM/dst.jpg");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_no_mkparent_when_dst_path_has_no_parent() {
    let client = fake_client();
    client.add_file("src.jpg", b"abc".to_vec());
    let backend = fuzzy_backend(client.clone());
    backend
        .copy_file(&mtp("src.jpg"), &mtp("dst.jpg"), true)
        .unwrap();
    // dst.jpg 无 parent：无 mkdir，dst.jpg 是 File
    let stored = client.get_file("dst.jpg");
    assert_eq!(stored.as_deref(), Some(&b"abc"[..]));
}

#[test]
fn copy_file_rejects_non_mtp_scheme_on_either_side() {
    let backend = fuzzy_backend(fake_client());
    let local = Location::Local(Utf8PathBuf::from("/tmp/x"));
    let err = backend.copy_file(&local, &mtp("dst"), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    let err = backend.copy_file(&mtp("src"), &local, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_propagates_read_error() {
    let client = fake_client();
    client.add_file("src.jpg", b"x".to_vec());
    client.inject(RemoteFakeOp::Read, "src.jpg", io::ErrorKind::Interrupted);
    let backend = fuzzy_backend(client);
    let err = backend
        .copy_file(&mtp("src.jpg"), &mtp("dst.jpg"), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}
