use std::fs;
use std::io;
use std::path::Path;

use camino::Utf8PathBuf;

use super::LocalBackend;
use super::noreplace::{rename_noreplace_with, reserve};
use crate::entities::backend::{Backend, is_partial_move, partial_move_error};
use crate::entities::uri::Location;

fn local(p: impl AsRef<Path>) -> Location {
    Location::Local(Utf8PathBuf::from_path_buf(p.as_ref().to_path_buf()).unwrap())
}

fn fail_plain(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other("boom"))
}

fn fail_partial(_: &Path, b: &Path) -> io::Result<()> {
    fs::write(b, b"copied").unwrap();
    Err(partial_move_error(
        io::ErrorKind::PermissionDenied,
        "copied x -> y but cannot remove source".to_owned(),
    ))
}

#[test]
fn reserve_fails_when_target_exists() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("a.bin");
    fs::write(&dst, b"keep").unwrap();
    let err = reserve(&dst).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&dst).unwrap(), b"keep");
}

#[test]
fn rename_refuses_to_replace_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, b"new").unwrap();
    fs::write(&dst, b"old").unwrap();
    let err = LocalBackend::new()
        .rename(&local(&src), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&src).unwrap(), b"new");
    assert_eq!(fs::read(&dst).unwrap(), b"old");
}

#[test]
fn rename_failure_removes_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, b"x").unwrap();
    let err = rename_noreplace_with(&src, &dst, fail_plain).unwrap_err();
    assert_eq!(err.to_string(), "boom");
    assert!(!dst.exists());
    assert!(src.exists());
}

#[test]
fn rename_partial_move_keeps_copied_target() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, b"x").unwrap();
    let err = rename_noreplace_with(&src, &dst, fail_partial).unwrap_err();
    assert!(is_partial_move(&err));
    assert_eq!(fs::read(&dst).unwrap(), b"copied");
}

#[test]
fn open_write_refuses_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("dst.bin");
    fs::write(&dst, b"old").unwrap();
    let Err(err) = LocalBackend::new().open_write(&local(&dst), false) else {
        panic!("open_write must refuse an existing target");
    };
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&dst).unwrap(), b"old");
}

#[test]
fn copy_file_refuses_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, b"new").unwrap();
    fs::write(&dst, b"old").unwrap();
    let err = LocalBackend::new()
        .copy_file(&local(&src), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&dst).unwrap(), b"old");
}

#[test]
fn copy_file_failure_removes_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("dst.bin");
    let err = LocalBackend::new()
        .copy_file(&local(dir.path().join("missing.bin")), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(!dst.exists());
}
