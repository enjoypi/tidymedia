//! `LocalBackend` 边界与 `copy_file` 测试：非 UTF-8 文件名 / socket / root-path /
//! chmod / copy 三态（从 `local_tests.rs` 拆出）。

use std::fs;
use std::io;

use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::LocalBackend;
use crate::entities::backend::Backend;
#[cfg(unix)]
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

fn local(p: impl AsRef<std::path::Path>) -> Location {
    Location::Local(Utf8PathBuf::from_path_buf(p.as_ref().to_path_buf()).unwrap())
}

fn smb_uri() -> Location {
    Location::parse("smb://nas/share/x").unwrap()
}

// Windows 文件名是 UTF-16，无法用任意字节构造非 UTF-8 路径
#[cfg(unix)]
#[test]
fn walk_entry_to_io_non_utf8_path() {
    use std::os::unix::ffi::OsStringExt;
    // ignore::WalkBuilder 拿到无效 UTF-8 路径时映射到 InvalidData。
    // 直接构造 ignore::DirEntry 不公开；我们走"在 tempdir 里创建一个含非 UTF-8 字节的文件名"
    let dir = tempdir().unwrap();
    let bad_name = std::ffi::OsString::from_vec(vec![0x66, 0x6f, 0xFF, 0x6f]); // f o \xFF o
    let path = dir.path().join(&bad_name);
    if fs::write(&path, b"x").is_err() {
        // 某些文件系统不允许非 UTF-8 文件名；跳过
        return;
    }
    let backend = LocalBackend::new();
    let entries: Vec<_> = backend.walk(&local(dir.path())).collect();
    let has_non_utf8_err = entries.iter().any(|r| {
        r.as_ref()
            .err()
            .is_some_and(|e| e.kind() == io::ErrorKind::InvalidData)
    });
    assert!(has_non_utf8_err);
}

#[test]
fn open_write_mkparents_fails_when_parent_is_file() {
    // parent 路径上已存在一个普通文件，create_dir_all 会因 ENOTDIR 失败
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("blocker"), b"i am a file").unwrap();
    let target = dir.path().join("blocker/x.bin");
    let err = LocalBackend::new()
        .open_write(&local(&target), true)
        .unwrap_err();
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
}

#[test]
fn copy_file_mkparents_fails_when_parent_is_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("a.bin");
    fs::write(&src, b"x").unwrap();
    fs::write(dir.path().join("blocker"), b"file").unwrap();
    let dst = dir.path().join("blocker/x.bin");
    let err = LocalBackend::new()
        .copy_file(&local(&src), &local(&dst), true)
        .unwrap_err();
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
}

#[test]
fn open_write_mkparents_root_path_no_parent() {
    // `/` 的 .parent() == None，走 if let 的 None 分支；之后 fs::File::create("/") 失败
    let root = Location::Local(Utf8PathBuf::from("/"));
    let err = LocalBackend::new().open_write(&root, true).unwrap_err();
    // 无法在 / 上创建文件——具体 ErrorKind 因平台而异，我们只要求确实 Err
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
}

#[test]
fn copy_file_mkparents_root_path_no_parent() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("a.bin");
    fs::write(&src, b"x").unwrap();
    let root = Location::Local(Utf8PathBuf::from("/"));
    let err = LocalBackend::new()
        .copy_file(&local(&src), &root, true)
        .unwrap_err();
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
}

// Unix domain socket 仅 Unix 可用
#[cfg(unix)]
#[test]
fn to_metadata_socket_returns_other_kind() {
    // UnixListener::bind 创建一个 socket 文件，is_file=false && is_dir=false
    use std::os::unix::net::UnixListener;
    let dir = tempdir().unwrap();
    let sock = dir.path().join("s.sock");
    let _l = UnixListener::bind(&sock).unwrap();
    let m = LocalBackend::new().metadata(&local(&sock)).unwrap();
    assert_eq!(m.kind, EntryKind::Other);
}

#[cfg(unix)]
#[test]
fn walk_socket_entry_kind_other() {
    use std::os::unix::net::UnixListener;
    let dir = tempdir().unwrap();
    let sock = dir.path().join("s.sock");
    let _l = UnixListener::bind(&sock).unwrap();
    let backend = LocalBackend::new();
    let entries: Vec<_> = backend
        .walk(&local(dir.path()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let any_other = entries.iter().any(|e| e.kind == EntryKind::Other);
    assert!(any_other);
}

// Windows 无 POSIX 权限位，chmod 000 无法模拟 PermissionDenied
#[cfg(unix)]
#[test]
fn open_read_chmod_000_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    fs::write(&path, b"data").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    let original = perms.mode();
    perms.set_mode(0o000);
    fs::set_permissions(&path, perms).unwrap();

    let err = LocalBackend::new().open_read(&local(&path)).unwrap_err();
    // chmod 000 通常映射到 PermissionDenied
    assert!(matches!(
        err.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::Other
    ));

    let mut restore = fs::metadata(&path).unwrap().permissions();
    restore.set_mode(original);
    fs::set_permissions(&path, restore).unwrap();
}

#[test]
fn copy_file_no_mkparents_writes_target() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("b.bin");
    fs::write(&src, b"data").unwrap();
    let n = LocalBackend::new()
        .copy_file(&local(&src), &local(&dst), false)
        .unwrap();
    assert_eq!(n, 4);
    assert_eq!(fs::read(&dst).unwrap(), b"data");
}

#[test]
fn copy_file_mkparents_creates_dir() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("sub/b.bin");
    fs::write(&src, b"data").unwrap();
    let n = LocalBackend::new()
        .copy_file(&local(&src), &local(&dst), true)
        .unwrap();
    assert_eq!(n, 4);
    assert_eq!(fs::read(&dst).unwrap(), b"data");
}

#[test]
fn copy_file_missing_source_not_found() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("missing");
    let dst = dir.path().join("out");
    let err = LocalBackend::new()
        .copy_file(&local(&src), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn copy_file_rejects_non_local_src_scheme() {
    let dir = tempdir().unwrap();
    let dst = dir.path().join("out");
    let err = LocalBackend::new()
        .copy_file(&smb_uri(), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_rejects_non_local_dst_scheme() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("a.bin");
    fs::write(&src, b"x").unwrap();
    let err = LocalBackend::new()
        .copy_file(&local(&src), &smb_uri(), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
