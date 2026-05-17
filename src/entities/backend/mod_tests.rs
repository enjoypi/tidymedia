use std::io::{self, Read, Seek, SeekFrom, Write};

use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;

use super::fake::{FakeBackend, Op};
use super::{Backend, Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

fn smb(path: &str) -> Location {
    Location::parse(&format!("smb://nas/share/{path}")).unwrap()
}

fn local(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

#[test]
fn entry_kind_equality_each_variant() {
    assert_eq!(EntryKind::File, EntryKind::File);
    assert_ne!(EntryKind::File, EntryKind::Dir);
    assert_ne!(EntryKind::Dir, EntryKind::Other);
    // Hash 派生：插 HashSet 验证可用
    let mut set = std::collections::HashSet::new();
    set.insert(EntryKind::File);
    set.insert(EntryKind::Dir);
    set.insert(EntryKind::Other);
    assert_eq!(set.len(), 3);
}

#[test]
fn entry_struct_fields() {
    let e = Entry {
        location: smb("a.jpg"),
        size: 42,
        kind: EntryKind::File,
    };
    assert_eq!(e.size, 42);
    assert_eq!(e.kind, EntryKind::File);
    // Debug 输出存在
    assert!(format!("{e:?}").contains("Entry"));
}

#[test]
fn metadata_struct_fields() {
    let m = Metadata {
        size: 7,
        kind: EntryKind::Dir,
        modified: Some(std::time::UNIX_EPOCH),
        created: None,
    };
    assert_eq!(m.size, 7);
    assert_eq!(m.kind, EntryKind::Dir);
    assert!(m.modified.is_some());
    assert!(m.created.is_none());
}

#[test]
fn fake_basic_add_and_metadata() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), b"hello".to_vec());
    let m = b.metadata(&loc).unwrap();
    assert_eq!(m.size, 5);
    assert_eq!(m.kind, EntryKind::File);
}

#[test]
fn fake_metadata_missing_returns_not_found() {
    let b = FakeBackend::new("smb");
    let err = b.metadata(&smb("missing")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn fake_metadata_inject_permission_denied() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), vec![0]);
    b.inject_error(loc.clone(), Op::Metadata, io::ErrorKind::PermissionDenied);
    let err = b.metadata(&loc).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn fake_exists_true_and_false() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    assert!(!b.exists(&loc).unwrap());
    b.add_file(loc.clone(), vec![0]);
    assert!(b.exists(&loc).unwrap());
}

#[test]
fn fake_exists_inject_other() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.inject_error(loc.clone(), Op::Exists, io::ErrorKind::Other);
    assert_eq!(b.exists(&loc).unwrap_err().kind(), io::ErrorKind::Other);
}

#[test]
fn fake_open_read_returns_bytes() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), b"hello world".to_vec());
    let mut r = b.open_read(&loc).unwrap();
    let mut s = String::new();
    r.read_to_string(&mut s).unwrap();
    assert_eq!(s, "hello world");
    // Seek 也支持
    r.seek(SeekFrom::Start(0)).unwrap();
    let mut s2 = String::new();
    r.read_to_string(&mut s2).unwrap();
    assert_eq!(s2, "hello world");
}

#[test]
fn fake_open_read_missing_not_found() {
    let b = FakeBackend::new("smb");
    let err = b.open_read(&smb("nope")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn fake_open_read_inject_permission_denied() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), vec![0]);
    b.inject_error(loc.clone(), Op::OpenRead, io::ErrorKind::PermissionDenied);
    assert_eq!(
        b.open_read(&loc).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn fake_open_write_then_finish_persists() {
    let b = FakeBackend::new("smb");
    let loc = smb("out.bin");
    let mut w = b.open_write(&loc, false).unwrap();
    w.write_all(b"abc").unwrap();
    w.flush().unwrap();
    w.finish().unwrap();
    assert_eq!(b.read_bytes(&loc).unwrap(), b"abc");
    assert_eq!(b.metadata(&loc).unwrap().size, 3);
}

#[test]
fn fake_open_write_inject_error() {
    let b = FakeBackend::new("smb");
    let loc = smb("out.bin");
    b.inject_error(loc.clone(), Op::OpenWrite, io::ErrorKind::PermissionDenied);
    assert_eq!(
        b.open_write(&loc, false).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn fake_walk_subset_under_root() {
    let b = FakeBackend::new("smb");
    let root = smb("dir");
    b.add_dir(root.clone());
    b.add_file(smb("dir/a.jpg"), b"a".to_vec());
    b.add_file(smb("dir/b/c.jpg"), b"c".to_vec());
    b.add_file(smb("other.jpg"), b"o".to_vec()); // 不在 root 下
    let mut out: Vec<String> = b
        .walk(&root)
        .map(|e| e.unwrap().location.display())
        .collect();
    out.sort();
    assert_eq!(
        out,
        vec![
            "smb://nas/share/dir".to_string(),
            "smb://nas/share/dir/a.jpg".to_string(),
            "smb://nas/share/dir/b/c.jpg".to_string(),
        ]
    );
}

#[test]
fn fake_walk_different_scheme_filtered() {
    let b = FakeBackend::new("local");
    b.add_file(local("/a/b.jpg"), b"x".to_vec());
    let smb_root = smb("dir");
    // root 与已添加文件 scheme 不同，walk 应返回空
    let out: Vec<_> = b.walk(&smb_root).collect();
    assert_eq!(out.len(), 0);
}

#[test]
fn fake_walk_inject_error() {
    let b = FakeBackend::new("smb");
    let root = smb("dir");
    b.inject_error(root.clone(), Op::Walk, io::ErrorKind::Other);
    let out: Vec<_> = b.walk(&root).collect();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].as_ref().unwrap_err().kind(), io::ErrorKind::Other);
}

#[test]
fn fake_remove_file_ok_and_not_found() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), vec![0]);
    b.remove_file(&loc).unwrap();
    assert!(!b.exists(&loc).unwrap());
    // 再删一次 → NotFound
    assert_eq!(
        b.remove_file(&loc).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn fake_remove_file_inject_error() {
    let b = FakeBackend::new("smb");
    let loc = smb("a.jpg");
    b.add_file(loc.clone(), vec![0]);
    b.inject_error(loc.clone(), Op::RemoveFile, io::ErrorKind::PermissionDenied);
    assert_eq!(
        b.remove_file(&loc).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn fake_mkdir_p_idempotent_and_inject() {
    let b = FakeBackend::new("smb");
    let loc = smb("dir");
    b.mkdir_p(&loc).unwrap();
    b.mkdir_p(&loc).unwrap();
    assert!(b.exists(&loc).unwrap());
    b.inject_error(loc.clone(), Op::MkdirP, io::ErrorKind::Other);
    assert_eq!(b.mkdir_p(&loc).unwrap_err().kind(), io::ErrorKind::Other);
}

#[test]
fn fake_read_to_string_ok_missing_and_inject() {
    let b = FakeBackend::new("smb");
    let loc = smb("note.txt");
    b.add_file(loc.clone(), b"hello".to_vec());
    assert_eq!(b.read_to_string(&loc).unwrap(), "hello");
    let missing = smb("nope.txt");
    assert_eq!(
        b.read_to_string(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    b.inject_error(loc.clone(), Op::ReadToString, io::ErrorKind::PermissionDenied);
    assert_eq!(
        b.read_to_string(&loc).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn fake_read_to_string_invalid_utf8() {
    let b = FakeBackend::new("smb");
    let loc = smb("bad.bin");
    b.add_file(loc.clone(), vec![0xFF, 0xFE]);
    assert_eq!(
        b.read_to_string(&loc).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn fake_copy_file_persists_and_errors() {
    let b = FakeBackend::new("smb");
    let src = smb("a.jpg");
    let dst = smb("b.jpg");
    b.add_file(src.clone(), b"data".to_vec());
    let n = b.copy_file(&src, &dst, false).unwrap();
    assert_eq!(n, 4);
    assert_eq!(b.read_bytes(&dst).unwrap(), b"data");
    // 源不存在
    let missing = smb("missing");
    assert_eq!(
        b.copy_file(&missing, &dst, false).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    // 注入错误
    b.inject_error(src.clone(), Op::CopyFile, io::ErrorKind::PermissionDenied);
    assert_eq!(
        b.copy_file(&src, &dst, false).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn fake_scheme_returns_init_value() {
    let b = FakeBackend::new("mtp");
    assert_eq!(b.scheme(), "mtp");
}

#[test]
fn inject_reader_error_returns_failing_reader() {
    let b = FakeBackend::new("fake");
    let loc = local("/in-mem/x.bin");
    b.add_file(loc.clone(), vec![1u8; 8]);
    b.inject_reader_error(loc.clone(), io::ErrorKind::Interrupted);

    let mut r = b.open_read(&loc).unwrap();
    let mut buf = [0u8; 4];
    let err = r.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    // Seek 始终返回 Ok(0)，让 `Box<dyn MediaReader>` 类型边界生效
    assert_eq!(r.seek(SeekFrom::Start(123)).unwrap(), 0);
}

#[test]
fn op_variants_distinct() {
    use Op::*;
    let all = [
        Metadata,
        Exists,
        Walk,
        OpenRead,
        OpenWrite,
        RemoveFile,
        MkdirP,
        ReadToString,
        CopyFile,
    ];
    // Debug 与 Hash + Eq 用 HashSet 验证去重
    let set: std::collections::HashSet<_> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len());
    // Debug 输出每个变体名
    for op in all {
        let _ = format!("{op:?}");
    }
}
