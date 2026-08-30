//! `RealSmbClient`：smb2（纯 Rust SMB2/3 客户端）适配器。
//!
//! 仅在 `--features smb-backend` 启用时编译。真实 SMB 调用需要 share 服务器，CI 不验证；
//! 调度层的 OK / Err 分支由 [`super::SmbBackend::with_client`] + `FakeSmbClient` 覆盖。
//!
//! ## async → sync 桥接
//!
//! smb2 全 async API（tokio）。本类型内嵌 `current_thread` runtime，6 个
//! `RemoteClient` 方法逐个 `runtime.block_on(...)` 桥接。`current_thread` 仅在
//! `block_on` 期间驱动内部任务，故 `auto_reconnect` 关闭——后台 reviver 任务在
//! 调用间隙停摆反而不可靠；断连让调用方拿到错误重跑。
//!
//! ## 线程安全
//!
//! `smb2::SmbClient` 方法全是 `&mut self`（连接内 credit/pipeline 状态），
//! [`parking_lot::Mutex`] 串行化。SmbClient/Tree 是纯 Rust async 类型（auto
//! `Send`），Mutex 包装后天然 `Send + Sync`——对比 pavao 时代的 raw `SMBCCTX`
//! 指针，无需 `unsafe impl`。
//!
//! ## 未覆盖的能力
//!
//! - Kerberos：`SmbTarget.krb5_ccname` 首期不消费（smb2 的 `KerberosAuthenticator`
//!   走独立 session 装配路径，未接入）；当前 NTLM username/password only。
//! - guest：`ClientConfig` 空 username/password 即 guest（库注释明示），未实测。

use std::io;
use std::time::Duration;

use camino::Utf8PathBuf;
use parking_lot::Mutex;

use super::super::remote::RemoteClient;
use super::SmbTarget;
use crate::entities::backend::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

pub struct RealSmbClient {
    runtime: tokio::runtime::Runtime,
    inner: Mutex<ShareSession>,
    user: Option<String>,
    host: String,
    port: Option<u16>,
    share: String,
}

struct ShareSession {
    client: smb2::SmbClient,
    tree: smb2::Tree,
}

impl std::fmt::Debug for RealSmbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealSmbClient")
            .field("host", &self.host)
            .field("share", &self.share)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl RealSmbClient {
    /// 从 [`SmbTarget`] 模板构造：host/port/share + env 凭据 + 配置项（外部传入）。
    /// `default_user` / `workgroup` / `timeout_secs` 由 factory 装配层从 `config()`
    /// 读取并传入，避免 entities 层反向依赖 `usecases::config`（CA 内层无依赖原则）。
    pub fn new(
        target: &SmbTarget,
        default_user: &str,
        workgroup: &str,
        timeout_secs: u64,
    ) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(io::Error::other)?;
        let user = target
            .user
            .clone()
            .unwrap_or_else(|| default_user.to_owned());
        let config = smb2::ClientConfig {
            addr: format!("{}:{}", target.host, target.port.unwrap_or(445)),
            timeout: Duration::from_secs(timeout_secs),
            username: user,
            password: target.password.clone().unwrap_or_default(),
            domain: workgroup.to_owned(),
            auto_reconnect: false,
            compression: false,
            dfs_enabled: true,
            dfs_target_overrides: std::collections::HashMap::new(),
        };
        let (client, tree) = runtime
            .block_on(async {
                let mut client = smb2::SmbClient::connect(config).await?;
                let tree = client.connect_share(&target.share).await?;
                Ok::<_, smb2::Error>((client, tree))
            })
            .map_err(map_smb_err)?;
        Ok(Self {
            runtime,
            inner: Mutex::new(ShareSession { client, tree }),
            user: target.user.clone(),
            host: target.host.clone(),
            port: target.port,
            share: target.share.clone(),
        })
    }

    fn child_target(&self, parent: &SmbTarget, name: &str) -> SmbTarget {
        // 直接 forward-slash 拼字符串，避免 Windows host 上 Utf8PathBuf::join 注入
        // `\` 进 SMB 路径（smb2 路径只认 `/`，反斜杠会被映射为 U+F026 普通字符
        // 致子路径查无此项）。对齐 adb_real::join_abs 单点 helper。
        let child_path = if parent.path.as_str().is_empty() {
            Utf8PathBuf::from(name)
        } else {
            let p = parent.path.as_str().trim_end_matches('/');
            Utf8PathBuf::from(format!("{p}/{name}"))
        };
        SmbTarget {
            user: self.user.clone(),
            host: self.host.clone(),
            port: self.port,
            share: self.share.clone(),
            path: child_path,
            password: parent.password.clone(),
            krb5_ccname: parent.krb5_ccname.clone(),
        }
    }
}

impl RemoteClient<SmbTarget> for RealSmbClient {
    fn stat(&self, target: &SmbTarget) -> io::Result<Metadata> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        let info = self
            .runtime
            .block_on(client.stat(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)?;
        Ok(Metadata {
            size: info.size,
            kind: if info.is_directory {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
            modified: info.modified.to_system_time(),
            created: info.created.to_system_time(),
        })
    }

    fn list(&self, target: &SmbTarget) -> io::Result<Vec<Entry>> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        let entries = self
            .runtime
            .block_on(client.list_directory(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            if e.name == "." || e.name == ".." {
                continue;
            }
            let kind = if e.is_directory {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            let child = self.child_target(target, &e.name);
            // smb2 DirectoryEntry 自带 size（QUERY_DIRECTORY 应答内含），免 pavao
            // 时代的逐 file 二次 stat RTT（F5 优化顺带落地）。
            out.push(Entry {
                location: smb_location_from_target(&child),
                size: e.size,
                kind,
            });
        }
        Ok(out)
    }

    fn read(&self, target: &SmbTarget) -> io::Result<Vec<u8>> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        // 整文件入堆（与 pavao 时代同口径的已知限制；F9 streaming 待做）。
        self.runtime
            .block_on(client.read_file(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }

    fn write(&self, target: &SmbTarget, data: &[u8]) -> io::Result<u64> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        // write_file 语义 = create or overwrite（对齐 pavao write+create+truncate）。
        self.runtime
            .block_on(client.write_file(tree, path_or_root(&target.path), data))
            .map_err(map_smb_err)
    }

    fn unlink(&self, target: &SmbTarget) -> io::Result<()> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.delete_file(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }

    fn mkdir(&self, target: &SmbTarget) -> io::Result<()> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.create_directory(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }
}

// share 根用 "."（smb2 对空 path 未定义行为）；子路径已是 `/` 拼接（child_target）。
fn path_or_root(p: &Utf8PathBuf) -> &str {
    if p.as_str().is_empty() {
        "."
    } else {
        p.as_str()
    }
}

// 用作 `.map_err(map_smb_err)` 回调，签名必须接收 owned error（map_err 传 owned），
// 故 needless_pass_by_value 在此不可消除。
#[expect(
    clippy::needless_pass_by_value,
    reason = "用作 map_err 回调，必须接收 owned smb2::Error"
)]
fn map_smb_err(e: smb2::Error) -> io::Error {
    // smb2 ErrorKind 精确分类（pavao 时代只能靠文案 to_ascii_lowercase 重映射）。
    let kind = match e.kind() {
        smb2::ErrorKind::NotFound => io::ErrorKind::NotFound,
        smb2::ErrorKind::AccessDenied
        | smb2::ErrorKind::AuthRequired
        | smb2::ErrorKind::SigningRequired => io::ErrorKind::PermissionDenied,
        smb2::ErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
        smb2::ErrorKind::TimedOut => io::ErrorKind::TimedOut,
        smb2::ErrorKind::ConnectionLost | smb2::ErrorKind::SessionExpired => {
            io::ErrorKind::ConnectionAborted
        }
        smb2::ErrorKind::DiskFull => io::ErrorKind::StorageFull,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("smb2: {e}"))
}

fn smb_location_from_target(t: &SmbTarget) -> Location {
    Location::Smb {
        user: t.user.clone(),
        host: t.host.clone(),
        port: t.port,
        share: t.share.clone(),
        path: t.path.clone(),
    }
}
