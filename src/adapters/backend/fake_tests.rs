use std::io;

use camino::Utf8PathBuf;

use super::{FakeBackend, Op};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

// rename default impl：copy_file + remove_file 两步，src 消失 dst 出现。
#[test]
fn rename_default_moves_file_via_copy_and_remove() {
    let b = FakeBackend::new("smb");
    let src = smb_loc("a.bin");
    let dst = smb_loc("b.bin");
    b.add_file(src.clone(), b"hello".to_vec());
    b.rename(&src, &dst, false).unwrap();
    assert!(b.read_bytes(&src).is_none(), "src must be removed");
    assert_eq!(b.read_bytes(&dst).as_deref(), Some(b"hello".as_ref()));
}

// copy_file 失败时 rename 传播错误，remove_file 不被调用（copy 提前返 Err）。
#[test]
fn rename_propagates_copy_error() {
    let b = FakeBackend::new("smb");
    let src = smb_loc("a.bin");
    let dst = smb_loc("b.bin");
    b.add_file(src.clone(), b"x".to_vec());
    b.inject_error(src.clone(), Op::CopyFile, io::ErrorKind::PermissionDenied);
    let err = b.rename(&src, &dst, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    // src 依然存在（copy 失败，remove 未执行）
    assert!(
        b.read_bytes(&src).is_some(),
        "src must remain on copy failure"
    );
}

// remove_file 失败时 rename 传播错误（copy 已成功，只有 remove 失败）。
// 默认 Backend::rename 必须把错误文案包装成 "copied … but cannot remove
// source"——让 do_copy / failed 计数能与 "copy 也失败" 区分，避免用户误判
// 重跑致丢源。
#[test]
fn rename_propagates_remove_error() {
    let b = FakeBackend::new("smb");
    let src = smb_loc("a.bin");
    let dst = smb_loc("b.bin");
    b.add_file(src.clone(), b"x".to_vec());
    b.inject_error(src.clone(), Op::RemoveFile, io::ErrorKind::PermissionDenied);
    let err = b.rename(&src, &dst, false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    let msg = err.to_string();
    // 拆成两条 assert：单 contains 让 LLVM 不为 `&&` 子分支拆 BR，避免 False
    // 路径（assert 失败即 test 失败）必然 0 hit 导致 branch coverage miss。
    assert!(msg.contains("copied"), "must mention 'copied', got: {msg}");
    assert!(
        msg.contains("but cannot remove source"),
        "must label half-state, got: {msg}"
    );
}
