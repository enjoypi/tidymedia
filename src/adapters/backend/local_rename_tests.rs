// `LocalBackend::rename` 单元测试。从 `local_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::fs;
use std::io;

use camino::Utf8PathBuf;

use super::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

fn local(p: impl AsRef<std::path::Path>) -> Location {
    Location::Local(Utf8PathBuf::from_path_buf(p.as_ref().to_path_buf()).unwrap())
}

fn smb_uri() -> Location {
    Location::parse("smb://nas/share/x").unwrap()
}

#[test]
fn rename_same_dir_moves_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("b.bin");
    fs::write(&src, b"move-me").unwrap();
    LocalBackend::new()
        .rename(&local(&src), &local(&dst), false)
        .unwrap();
    assert!(!src.exists(), "src must be gone after rename");
    assert_eq!(fs::read(&dst).unwrap(), b"move-me");
}

#[test]
fn rename_mkparents_creates_dir() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("sub/dst.bin");
    fs::write(&src, b"x").unwrap();
    LocalBackend::new()
        .rename(&local(&src), &local(&dst), true)
        .unwrap();
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"x");
}

#[test]
fn rename_missing_source_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let err = LocalBackend::new()
        .rename(
            &local(dir.path().join("missing.bin")),
            &local(dir.path().join("dst.bin")),
            false,
        )
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn rename_rejects_non_local_from_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("dst.bin");
    let err = LocalBackend::new()
        .rename(&smb_uri(), &local(&dst), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn rename_rejects_non_local_to_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    fs::write(&src, b"x").unwrap();
    let err = LocalBackend::new()
        .rename(&local(&src), &smb_uri(), false)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn rename_mkparents_root_path_no_parent() {
    // to=`/`，parent() 返 None，走 if-let 的 None 分支
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    fs::write(&src, b"x").unwrap();
    let root = Location::Local(Utf8PathBuf::from("/"));
    let err = LocalBackend::new()
        .rename(&local(&src), &root, true)
        .unwrap_err();
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
}

/// parent 路径上恰好是一个普通文件 → `fs::create_dir_all` 返 `NotADirectory` Err，
/// `rename` 必须透传该 Err（覆盖 local.rs:119 mkparents 下 `?` 的失败分支）。
#[test]
fn rename_mkparents_propagates_create_dir_all_error() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    fs::write(&src, b"x").unwrap();
    // 在 tempdir 内放一个普通文件 blocker；让 dst 的 parent 指向它。
    fs::write(dir.path().join("blocker"), b"i am a file").unwrap();
    let dst = dir.path().join("blocker/inside.bin");
    let err = LocalBackend::new()
        .rename(&local(&src), &local(&dst), true)
        .unwrap_err();
    // 平台映射到 NotADirectory（unix）或 Other；只要不是 Unsupported 即认为 ? 正确传播。
    assert!(!matches!(err.kind(), io::ErrorKind::Unsupported));
    // src 必须仍在，rename 未发生
    assert!(src.exists(), "rename failed before move, src must remain");
}
