use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::super::{Backend, EntryKind};
use super::LocalBackend;
use crate::entities::uri::Location;

fn local(p: impl AsRef<std::path::Path>) -> Location {
    Location::Local(Utf8PathBuf::from_path_buf(p.as_ref().to_path_buf()).unwrap())
}

fn smb_uri() -> Location {
    Location::parse("smb://nas/share/x").unwrap()
}

#[test]
fn scheme_is_local() {
    assert_eq!(LocalBackend::new().scheme(), "local");
}

#[test]
fn arc_factory_returns_dyn_backend() {
    let b = LocalBackend::arc();
    assert_eq!(b.scheme(), "local");
}

#[test]
fn metadata_returns_file_kind_and_size() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    fs::write(&path, b"hello").unwrap();
    let m = LocalBackend::new().metadata(&local(&path)).unwrap();
    assert_eq!(m.size, 5);
    assert_eq!(m.kind, EntryKind::File);
}

#[test]
fn metadata_dir_kind() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let m = LocalBackend::new().metadata(&local(&sub)).unwrap();
    assert_eq!(m.kind, EntryKind::Dir);
}

#[test]
fn metadata_missing_path_returns_not_found() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing");
    let err = LocalBackend::new().metadata(&local(&path)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn metadata_rejects_non_local_scheme() {
    let err = LocalBackend::new().metadata(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn exists_true_false() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    assert!(!LocalBackend::new().exists(&local(&path)).unwrap());
    fs::write(&path, b"x").unwrap();
    assert!(LocalBackend::new().exists(&local(&path)).unwrap());
}

#[test]
fn exists_rejects_non_local_scheme() {
    let err = LocalBackend::new().exists(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_finds_files_under_root() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.bin"), b"a").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/b.bin"), b"b").unwrap();
    let backend = LocalBackend::new();
    let entries: Vec<_> = backend
        .walk(&local(dir.path()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    // 至少包含 a.bin / sub / sub/b.bin
    let names: Vec<String> = entries
        .iter()
        .map(|e| match &e.location {
            Location::Local(p) => p.file_name().unwrap_or("").to_string(),
            _ => String::new(),
        })
        .collect();
    assert!(names.contains(&"a.bin".to_string()));
    assert!(names.contains(&"sub".to_string()));
    assert!(names.contains(&"b.bin".to_string()));
}

#[test]
fn walk_rejects_non_local_scheme() {
    let backend = LocalBackend::new();
    let mut it = backend.walk(&smb_uri());
    let first = it.next().unwrap().unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_propagates_ignore_io_error() {
    // 父目录权限 0o000，ignore 会在 read_dir 失败
    let dir = tempdir().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("a.bin"), b"a").unwrap();
    let mut perms = fs::metadata(&sub).unwrap().permissions();
    let original = perms.mode();
    perms.set_mode(0o000);
    fs::set_permissions(&sub, perms).unwrap();

    let backend = LocalBackend::new();
    let mut found_err = false;
    for r in backend.walk(&local(&sub)) {
        if r.is_err() {
            found_err = true;
            break;
        }
    }
    // 恢复权限避免 tempdir 清理失败
    let mut restore = fs::metadata(&sub).unwrap().permissions();
    restore.set_mode(original);
    fs::set_permissions(&sub, restore).unwrap();

    assert!(found_err, "expected at least one io::Error from walk");
}

#[test]
fn open_read_reads_bytes_and_seeks() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    fs::write(&path, b"abcdef").unwrap();
    let backend = LocalBackend::new();
    let mut r = backend.open_read(&local(&path)).unwrap();
    let mut buf = String::new();
    r.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "abcdef");
    r.seek(SeekFrom::Start(2)).unwrap();
    let mut buf2 = String::new();
    r.read_to_string(&mut buf2).unwrap();
    assert_eq!(buf2, "cdef");
}

#[test]
fn open_read_missing_not_found() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing");
    let err = LocalBackend::new().open_read(&local(&path)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn open_read_rejects_non_local_scheme() {
    let err = LocalBackend::new().open_read(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_no_mkparents_then_finish() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("out.bin");
    let backend = LocalBackend::new();
    let mut w = backend.open_write(&local(&path), false).unwrap();
    w.write_all(b"hi").unwrap();
    w.flush().unwrap();
    w.finish().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"hi");
}

#[test]
fn open_write_mkparents_creates_dir() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a/b/c.bin");
    let backend = LocalBackend::new();
    let mut w = backend.open_write(&local(&path), true).unwrap();
    w.write_all(b"x").unwrap();
    w.finish().unwrap();
    assert!(path.exists());
}

#[test]
fn open_write_no_mkparents_fails_when_parent_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nope/x.bin");
    let err = LocalBackend::new()
        .open_write(&local(&path), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn open_write_rejects_non_local_scheme() {
    let err = LocalBackend::new()
        .open_write(&smb_uri(), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn remove_file_ok_and_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    fs::write(&path, b"x").unwrap();
    let backend = LocalBackend::new();
    backend.remove_file(&local(&path)).unwrap();
    assert!(!path.exists());
    let err = backend.remove_file(&local(&path)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn remove_file_rejects_non_local_scheme() {
    let err = LocalBackend::new().remove_file(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a/b/c");
    let backend = LocalBackend::new();
    backend.mkdir_p(&local(&path)).unwrap();
    backend.mkdir_p(&local(&path)).unwrap();
    assert!(path.is_dir());
}

#[test]
fn mkdir_p_rejects_non_local_scheme() {
    let err = LocalBackend::new().mkdir_p(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_ok_and_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("n.txt");
    fs::write(&path, b"hello").unwrap();
    let backend = LocalBackend::new();
    assert_eq!(backend.read_to_string(&local(&path)).unwrap(), "hello");
    let missing = dir.path().join("missing");
    assert_eq!(
        backend
            .read_to_string(&local(&missing))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn read_to_string_rejects_non_local_scheme() {
    let err = LocalBackend::new().read_to_string(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
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

#[test]
fn open_read_chmod_000_permission_denied() {
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
