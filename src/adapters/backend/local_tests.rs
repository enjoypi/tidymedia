use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::LocalBackend;
use crate::entities::backend::{Backend, EntryKind};
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

// Windows 无 POSIX 权限位，chmod 0o000 无法模拟 read_dir 失败
#[cfg(unix)]
#[test]
fn walk_propagates_ignore_io_error() {
    use std::os::unix::fs::PermissionsExt;
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
        backend.read_to_string(&local(&missing)).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn read_to_string_rejects_non_local_scheme() {
    let err = LocalBackend::new().read_to_string(&smb_uri()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
