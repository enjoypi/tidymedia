//! `SmbBackend` 内部 helpers / 适配器测试：`map_error`、`BufferedWriter`、
//! `parent_target`、`build_target`、`SmbTarget` impl、`arc_with_client`。
//! 从 `smb_tests.rs` 拆出避免单文件 > 512 行。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use super::*;
use crate::entities::backend::EntryKind;
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

// ── mkdir_p 递归（mkdir_recursive 经 SmbAdapter 的行为锚定）────────────────

// mkdir_p 对多层路径必须逐层创建：smb2 `create_directory` 是单层语义，父层缺失返
// ENOENT；默认 archive_template 渲染出 `{year}/{month}` 两层，旧实现仅叶节点
// 一次 mkdir 在真实 SMB 上必败（fake 的 mkdir 不校验父目录曾掩盖该缺陷）。
#[test]
fn mkdir_p_creates_intermediate_layers() {
    let client = fake_client();
    let backend = backend_with(client.clone());
    backend.mkdir_p(&smb("2024/01")).unwrap();
    let parent = client
        .get_metadata("2024")
        .expect("parent layer must be created");
    assert_eq!(parent.kind, EntryKind::Dir);
    let leaf = client
        .get_metadata("2024/01")
        .expect("leaf layer must be created");
    assert_eq!(leaf.kind, EntryKind::Dir);
}

// 中间层 mkdir 失败必须传播（证明父层 mkdir 确实被调用且错误不被吞）。
#[test]
fn mkdir_p_propagates_intermediate_mkdir_error() {
    let client = fake_client();
    client.inject(RemoteFakeOp::Mkdir, "2024", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.mkdir_p(&smb("2024/01")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

// 并发/重复创建产生的 AlreadyExists 必须被容忍。
#[test]
fn mkdir_p_tolerates_already_exists() {
    let client = fake_client();
    client.inject(RemoteFakeOp::Mkdir, "2024/01", io::ErrorKind::AlreadyExists);
    let backend = backend_with(client);
    backend.mkdir_p(&smb("2024/01")).unwrap();
}

// stat 的非 NotFound 错误（网络故障）直接传播，不在故障链路上盲目 mkdir。
#[test]
fn mkdir_p_propagates_non_notfound_stat_error() {
    let client = fake_client();
    client.inject(RemoteFakeOp::Stat, "2024/01", io::ErrorKind::TimedOut);
    let backend = backend_with(client);
    let err = backend.mkdir_p(&smb("2024/01")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn seek_position_supports_all_modes_and_rejects_negative() {
    assert_eq!(seek_position(10, 100, io::SeekFrom::Start(20)).unwrap(), 20);
    assert_eq!(
        seek_position(10, 100, io::SeekFrom::Current(-4)).unwrap(),
        6
    );
    assert_eq!(seek_position(10, 100, io::SeekFrom::End(-5)).unwrap(), 95);
    assert!(seek_position(0, 100, io::SeekFrom::Current(-1)).is_err());
    assert!(seek_position(0, 100, io::SeekFrom::End(-101)).is_err());
    assert!(seek_position(u64::MAX, 100, io::SeekFrom::Current(1)).is_err());
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

// pavao 把"路径不存在"包成 `io::Error::other("pavao: ENOENT ...")`：map_error 必须
// 把这种文本错误归一为 NotFound，否则 mkdir_recursive 的 NotFound guard 永不触发
// → 多层归档目录创建必败（生产场景 fake 返干净 NotFound 掩盖该缺陷）。
#[test]
fn map_smb_error_other_with_enoent_text_to_notfound() {
    let e = io::Error::other("pavao: ENOENT No such file or directory");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_smb_error_other_with_no_such_file_text_to_notfound() {
    let e = io::Error::other("smb: no such file");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_smb_error_other_with_does_not_exist_text_to_notfound() {
    let e = io::Error::other("share does not exist on host");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

// 三个 `contains` 子分支各自 True 路径覆盖：EEXIST / file exists / already exists 文案
// → AlreadyExists kind。避免 `||` 短路让首个 contains 永远先命中、后两个子分支 0 hit。
#[test]
fn map_smb_error_other_with_eexist_text_to_already_exists() {
    let e = io::Error::other("pavao: EEXIST resource already there");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn map_smb_error_other_with_file_exists_text_to_already_exists() {
    let e = io::Error::other("smb: File exists");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn map_smb_error_other_with_already_exists_text_to_already_exists() {
    let e = io::Error::other("smb client: object Already Exists");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn map_smb_error_other_with_permission_text_to_permission_denied() {
    let e = io::Error::other("permission denied while opening share");
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

// 已正确分类的 kind 必须直接放行；防 future pavao 版本提前归一时被文案重映射
// 误覆盖。
#[test]
fn map_smb_error_already_notfound_passthrough() {
    let e = io::Error::from(io::ErrorKind::NotFound);
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
}

#[test]
fn map_smb_error_already_permission_denied_passthrough() {
    let e = io::Error::from(io::ErrorKind::PermissionDenied);
    let mapped = SmbAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::PermissionDenied);
}

// mkdir_p 端到端：pavao 风格 ENOENT 文案的 stat 错误必须被 map_error 归一为
// NotFound，让 mkdir_recursive 自底向上扫描祖先后逐层 mkdir 成功。该测试覆盖
// 生产 SMB 多层归档目录创建路径，弥补既有 fake_client 干净 NotFound 的覆盖盲区。
#[test]
fn mkdir_p_succeeds_when_stat_returns_pavao_style_enoent_text() {
    let client = Arc::new(FakeClient::with_error_factory(|k| match k {
        io::ErrorKind::NotFound => io::Error::other("pavao: ENOENT No such file or directory"),
        other => io::Error::from(other),
    }));
    let backend = backend_with(client.clone());
    backend.mkdir_p(&smb("2024/01")).unwrap();
    let parent = client
        .get_metadata("2024")
        .expect("parent layer must be created");
    assert_eq!(parent.kind, EntryKind::Dir);
    let leaf = client
        .get_metadata("2024/01")
        .expect("leaf layer must be created");
    assert_eq!(leaf.kind, EntryKind::Dir);
}

#[test]
fn smb_buffered_writer_debug_format() {
    use std::io::Write;
    let client = fake_client();
    let backend = backend_with(client);
    let mut w = backend.open_write(&smb("x.bin"), false).unwrap();
    w.write_all(b"abc").unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("FakeRemoteWriter"), "got: {s}");
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

include!("smb_more_tests.rs");
