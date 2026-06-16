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

// `rename_or_fallback_with` 的 CrossesDevices fallback：注入 mock rename 返
// `CrossesDevices` Err 触发 copy + remove。ecs-user 无 mount 权限不能真造跨设备，
// 用 fn pointer 注入测。
fn rename_cross_devices_mock(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::CrossesDevices))
}

#[test]
fn rename_or_fallback_uses_copy_and_remove_on_crosses_devices() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    fs::write(&src, b"hello").unwrap();
    super::rename_or_fallback_with(
        &src,
        &dst,
        rename_cross_devices_mock,
        super::real_copy,
        super::real_remove,
    )
    .unwrap();
    assert!(dst.is_file());
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
}

#[test]
fn rename_or_fallback_propagates_non_crosses_devices_err() {
    fn rename_perm_denied(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
    let err = super::rename_or_fallback_with(
        std::path::Path::new("/x"),
        std::path::Path::new("/y"),
        rename_perm_denied,
        super::real_copy,
        super::real_remove,
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

// CrossesDevices fallback 中 copy 失败传播：触发 `copy(from, to)?` 的 `?` Err arm。
fn copy_fails_mock(_: &std::path::Path, _: &std::path::Path) -> io::Result<u64> {
    Err(io::Error::other("inject copy fail"))
}

#[test]
fn rename_or_fallback_propagates_copy_err_in_fallback() {
    let err = super::rename_or_fallback_with(
        std::path::Path::new("/x"),
        std::path::Path::new("/y"),
        rename_cross_devices_mock,
        copy_fails_mock,
        super::real_remove,
    )
    .unwrap_err();
    assert!(err.to_string().contains("inject copy fail"), "{err}");
}

// walk_entry_to_io 中 entry.metadata() Err 触发 "metadata failed" 文案。
// 真实 ignore::DirEntry.metadata() 仅在并发删除等罕见情况失败，用 fn pointer 注入 mock。
fn meta_fail_mock(_entry: &ignore::DirEntry) -> Result<std::fs::Metadata, ignore::Error> {
    Err(ignore::Error::Io(io::Error::other("inject meta fail")))
}

#[test]
fn walk_entry_to_io_propagates_metadata_failure() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.bin"), b"x").unwrap();
    let entry = ignore::WalkBuilder::new(dir.path())
        .build()
        .filter_map(Result::ok)
        .find(|e| e.file_type().is_some_and(|t| t.is_file()))
        .expect("real DirEntry");
    let err = super::walk_entry_to_io_with(Ok(entry), meta_fail_mock).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("metadata failed for"), "got: {msg}");
    assert!(
        msg.contains("inject meta fail"),
        "must wrap inner err: {msg}"
    );
}

// CrossesDevices fallback 中 copy 成功但 remove 失败：触发 `remove(from).map_err(|re| ...)`
// 闭包，验证 "copied … but cannot remove source" 半态文案。
fn remove_fails_mock(_: &std::path::Path) -> io::Result<()> {
    Err(io::Error::other("inject remove fail"))
}

#[test]
fn rename_or_fallback_marks_half_state_when_remove_fails_in_fallback() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    fs::write(&src, b"hello").unwrap();
    let err = super::rename_or_fallback_with(
        &src,
        &dst,
        rename_cross_devices_mock,
        super::real_copy,
        remove_fails_mock,
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("copied") && msg.contains("but cannot remove source"),
        "half-state must be labelled, got: {msg}"
    );
    assert!(
        msg.contains("inject remove fail"),
        "must wrap original error: {msg}"
    );
}

// `ignore_to_io` 处理 ignore::Error 的 io_error()==None 分支（如 Loop variant，
// symlink loop 不易在 CI 稳定触发；直接构造 enum variant 测 helper）。
#[test]
fn ignore_to_io_handles_loop_variant_with_no_io_error() {
    let loop_err = ignore::Error::Loop {
        ancestor: std::path::PathBuf::from("/a"),
        child: std::path::PathBuf::from("/a/b"),
    };
    let io_err = super::ignore_to_io(&loop_err);
    // io_error() 为 None → 走 io::Error::other 分支，kind 是 Other
    assert_eq!(io_err.kind(), io::ErrorKind::Other);
}

#[test]
fn ignore_to_io_handles_io_variant_propagates_kind() {
    let inner = io::Error::from(io::ErrorKind::PermissionDenied);
    let ig = ignore::Error::Io(inner);
    let io_err = super::ignore_to_io(&ig);
    assert_eq!(io_err.kind(), io::ErrorKind::PermissionDenied);
}

// `open_read_inner_with` 注入 mock mmap_fn 触发 line 247 mmap `?` Err arm。
fn mock_mmap_fails(_: &fs::File) -> io::Result<memmap2::Mmap> {
    Err(io::Error::other("inject mmap fail"))
}

#[test]
fn open_read_inner_propagates_mmap_err() {
    let dir = tempdir().unwrap();
    let f = dir.path().join("ok.bin");
    fs::write(&f, b"hello").unwrap();
    let err = super::open_read_inner_with(&f, mock_mmap_fails).unwrap_err();
    assert!(err.to_string().contains("inject mmap fail"), "{err}");
}
