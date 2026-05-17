//! `RealSmbClient`：pavao + libsmbclient 适配器。
//!
//! 仅在 `--features smb-backend` 启用时编译。**整模块标 `#[cfg_attr(coverage_nightly,
//! coverage(off))]`**：真实 SMB 调用需要 share 服务器才能稳定触发，CI 无法覆盖。
//! 调度层的 OK / Err 分支由 [`super::SmbBackend::with_client`] + FakeSmbClient 覆盖。
//!
//! ## 线程安全
//!
//! `pavao::SmbClient` 内部持 libsmbclient 的 raw `SMBCCTX` 指针，未声明 `Send + Sync`。
//! 该 C 句柄在多线程并发使用时不安全（参见 Samba 文档），因此 `RealSmbClient` 用
//! [`parking_lot::Mutex`] 串行化所有调用，并对外声明 `unsafe impl Send + Sync`。
//! 调用方层（`SmbBackend` / Use Case 的 `par_iter`）能放心 `Arc<dyn SmbClient>` 跨线程。
//!
//! ## 未覆盖的能力
//!
//! - Kerberos：当前只支持 username/password（pavao 0.2 暴露面有限）；`KRB5CCNAME`
//!   走环境变量由 libsmbclient 自动拾取，无显式 API。
//! - timeout：`SmbOptions` 没显式 timeout，本 PR 未接入；`config.backend.smb.timeout_secs`
//!   暂仅作配置占位。

#![cfg_attr(coverage_nightly, coverage(off))]

use std::io;
use std::time::SystemTime;

use parking_lot::Mutex;
use pavao::{
    SmbClient as PavaoClient, SmbCredentials, SmbDirentType, SmbMode, SmbOpenOptions, SmbOptions,
};

use super::{SmbClient, SmbTarget};
use super::super::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

pub struct RealSmbClient {
    inner: Mutex<PavaoClient>,
    /// `smb://[host][:port]/share`，url_for 在末尾拼 path。
    share_url: String,
    user: Option<String>,
    host: String,
    port: Option<u16>,
    share: String,
}

// libsmbclient ctx 内部用 raw pointer + 全局 state；包了 Mutex 之后所有调用串行化，
// 因此对外可以安全 Send + Sync。该 unsafe impl 是必要的：pavao 0.2 不主动 derive 这两个 trait。
unsafe impl Send for RealSmbClient {}
unsafe impl Sync for RealSmbClient {}

impl std::fmt::Debug for RealSmbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealSmbClient")
            .field("share_url", &self.share_url)
            .field("user", &self.user)
            .finish()
    }
}

impl RealSmbClient {
    /// 从 [`SmbTarget`] 模板构造：host/port/share + env 凭据 + 配置项（外部传入）。
    /// `default_user` / `workgroup` 由 lib.rs 装配层从 config() 读取并传入，避免
    /// entities 层反向依赖 usecases::config（Clean Architecture 内层无依赖原则）。
    pub fn new(target: &SmbTarget, default_user: &str, workgroup: &str) -> io::Result<Self> {
        let server = format!(
            "smb://{}{}",
            target.host,
            target
                .port
                .map(|p| format!(":{p}"))
                .unwrap_or_default()
        );
        let user = target
            .user
            .clone()
            .unwrap_or_else(|| default_user.to_string());
        let password = target.password.clone().unwrap_or_default();
        let workgroup = workgroup.to_string();
        let creds = SmbCredentials::default()
            .server(&server)
            .share(format!("/{}", target.share))
            .username(user.clone())
            .password(password)
            .workgroup(workgroup);
        let opts = SmbOptions::default()
            .case_sensitive(true)
            .one_share_per_server(true);
        let client = PavaoClient::new(creds, opts)
            .map_err(|e| io::Error::other(format!("pavao SmbClient::new: {e}")))?;
        let share_url = format!("{server}/{}", target.share);
        Ok(Self {
            inner: Mutex::new(client),
            share_url,
            user: target.user.clone(),
            host: target.host.clone(),
            port: target.port,
            share: target.share.clone(),
        })
    }

    fn url_for(&self, target: &SmbTarget) -> String {
        if target.path.as_str().is_empty() {
            self.share_url.clone()
        } else {
            format!("{}/{}", self.share_url, target.path)
        }
    }

    fn child_target(&self, parent: &SmbTarget, name: &str) -> SmbTarget {
        let child_path = if parent.path.as_str().is_empty() {
            camino::Utf8PathBuf::from(name)
        } else {
            parent.path.join(name)
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

impl SmbClient for RealSmbClient {
    fn stat(&self, target: &SmbTarget) -> io::Result<Metadata> {
        let url = self.url_for(target);
        let s = self.inner.lock().stat(&url).map_err(map_smb_err)?;
        Ok(Metadata {
            size: s.size,
            kind: kind_from_mode(&s.mode),
            modified: Some(s.modified),
            created: Some(s.created),
        })
    }

    fn list(&self, target: &SmbTarget) -> io::Result<Vec<Entry>> {
        let url = self.url_for(target);
        let entries = self.inner.lock().list_dir(&url).map_err(map_smb_err)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            let name = e.name();
            if name == "." || name == ".." {
                continue;
            }
            let kind = kind_from_dirent(e.get_type());
            let child = self.child_target(target, name);
            // list_dir 不带 size：file 时再 stat 一次；目录给 0（visit_location 只对 file 看 size）。
            let size = if matches!(kind, EntryKind::File) {
                self.stat(&child).map(|m| m.size).unwrap_or(0)
            } else {
                0
            };
            out.push(Entry {
                location: smb_location_from_target(&child),
                size,
                kind,
            });
        }
        Ok(out)
    }

    fn read(&self, target: &SmbTarget) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let url = self.url_for(target);
        let guard = self.inner.lock();
        let mut file = guard
            .open_with(&url, SmbOpenOptions::default().read(true))
            .map_err(map_smb_err)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write(&self, target: &SmbTarget, data: &[u8]) -> io::Result<u64> {
        use std::io::Write;
        let url = self.url_for(target);
        let guard = self.inner.lock();
        let mut file = guard
            .open_with(
                &url,
                SmbOpenOptions::default()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o644),
            )
            .map_err(map_smb_err)?;
        file.write_all(data)?;
        file.flush()?;
        Ok(data.len() as u64)
    }

    fn unlink(&self, target: &SmbTarget) -> io::Result<()> {
        let url = self.url_for(target);
        self.inner.lock().unlink(&url).map_err(map_smb_err)
    }

    fn mkdir(&self, target: &SmbTarget) -> io::Result<()> {
        let url = self.url_for(target);
        self.inner
            .lock()
            .mkdir(&url, SmbMode::from(0o755u32))
            .map_err(map_smb_err)
    }
}

fn map_smb_err(e: pavao::SmbError) -> io::Error {
    io::Error::other(format!("pavao: {e}"))
}

fn kind_from_mode(m: &SmbMode) -> EntryKind {
    if m.is_dir() {
        EntryKind::Dir
    } else if m.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn kind_from_dirent(t: SmbDirentType) -> EntryKind {
    match t {
        SmbDirentType::File => EntryKind::File,
        SmbDirentType::Dir => EntryKind::Dir,
        _ => EntryKind::Other,
    }
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

/// 时间戳静态使用：避免 SystemTime 在某些平台下未实现 `Default`。
#[allow(dead_code)]
fn epoch_fallback() -> SystemTime {
    SystemTime::UNIX_EPOCH
}
