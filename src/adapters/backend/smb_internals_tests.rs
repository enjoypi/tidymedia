//! `SmbBackend` 内部 helpers / 适配器测试：`map_error`、`BufferedWriter`、
//! `parent_target`、`build_target`、`SmbTarget` impl、`arc_with_client`。
//! 从 `smb_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

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

// mkdir_p 对多层路径必须逐层创建：pavao 的 mkdir 是 POSIX 单层语义，父层缺失返
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
// 误覆盖（与 AdbAdapter::map_error 同一守门策略）。
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
    assert!(s.contains("RemoteBufferedWriter"), "got: {s}");
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

#[test]
fn parent_target_returns_none_for_root_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("only.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b/c.bin"),
        password: None,
        krb5_ccname: None,
    };
    let p = t.parent().unwrap();
    assert_eq!(p.path.as_str(), "a/b");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    // Utf8PathBuf::from("x.bin").parent() == Some("")，要走 if-empty 早返回
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn build_target_threads_env_password_and_krb5() {
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::set_var("SMB_PASSWORD", "secret-pw");
        std::env::set_var("KRB5CCNAME", "/tmp/krb5cc_0");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert_eq!(t.password.as_deref(), Some("secret-pw"));
    assert_eq!(t.krb5_ccname.as_deref(), Some("/tmp/krb5cc_0"));
    assert_eq!(t.user.as_deref(), Some("alice"));
    assert_eq!(t.host, "nas");
    assert_eq!(t.port, Some(445));
    assert_eq!(t.share, "photos");
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
}

#[test]
fn build_target_leaves_password_none_when_env_unset() {
    // SAFETY: nextest 每测试独立进程
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert!(t.password.is_none());
    assert!(t.krb5_ccname.is_none());
}

#[test]
fn smb_target_equality_and_debug() {
    let t1 = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x"),
        password: None,
        krb5_ccname: None,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    // 空字符串 path 让 Utf8Path::parent() 直接返 None，命中 `?` early return
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from(""),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn SmbClient> = fake_client();
    let backend = SmbBackend::arc_with_client(client);
    assert_eq!(backend.scheme(), "smb");
}

#[test]
fn smb_target_entry_location_constructs_uri() {
    let t = SmbTarget {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    let loc = t.entry_location(Utf8PathBuf::from("x/y.jpg"));
    assert_eq!(
        loc,
        Location::Smb {
            user: Some("alice".into()),
            host: "nas".into(),
            port: Some(445),
            share: "photos".into(),
            path: Utf8PathBuf::from("x/y.jpg"),
        }
    );
}

#[test]
fn smb_target_path_returns_inner_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    assert_eq!(t.path().as_str(), "a/b.jpg");
}
