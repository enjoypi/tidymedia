//! SMB Backend：把 `smb://[user@]host[:port]/share/path` 形态的 [`Location`]
//! 转成 [`SmbClient`] trait 上的调用，client 自身可注入。
//!
//! ## 真实库连接
//! - 真实 `smb` crate 适配器（[`RealSmbClient`]）留作未来 PR 接入；当前 `SmbBackend::new()`
//!   返 [`io::ErrorKind::Unsupported`] "smb-backend not enabled"，对应 plan §9 风险 1
//!   的"feature 未启用回滚边界"。
//! - 单元测试通过 [`SmbBackend::with_client`] 注入 fake 验证：URI 解析、env 凭据传递、
//!   error 映射等调度逻辑可在不依赖真实 SMB server 的前提下 100% 覆盖。
//!
//! ## 环境变量
//! - `SMB_PASSWORD`：URI 中没有 `user@` 时由 `default_user` 兜底；密码总是经环境变量
//! - `KRB5CCNAME`：Kerberos ticket cache 路径（让 client 走 SSO）
//!
//! 详见 CLAUDE.md「URI 与 Backend」段。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

/// SMB target 的最小可识别参数集。`SmbClient` 实现按此参数访问远端。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub share: String,
    pub path: Utf8PathBuf,
    /// 经环境变量 `SMB_PASSWORD` 注入；URI 中不暴露明文密码。
    pub password: Option<String>,
    /// Kerberos ticket cache 路径，对应环境变量 `KRB5CCNAME`。
    pub krb5_ccname: Option<String>,
}

/// SMB 协议客户端抽象。`SmbBackend` 持 `Box<dyn SmbClient>`，让真实库适配器与
/// fake 测试 client 等价替换。
pub trait SmbClient: Send + Sync + std::fmt::Debug {
    fn stat(&self, target: &SmbTarget) -> io::Result<Metadata>;
    fn list(&self, target: &SmbTarget) -> io::Result<Vec<Entry>>;
    fn read(&self, target: &SmbTarget) -> io::Result<Vec<u8>>;
    fn write(&self, target: &SmbTarget, data: &[u8]) -> io::Result<u64>;
    fn unlink(&self, target: &SmbTarget) -> io::Result<()>;
    fn mkdir(&self, target: &SmbTarget) -> io::Result<()>;
}

pub struct SmbBackend {
    client: Arc<dyn SmbClient>,
}

impl SmbBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported` 让上层报错。
    /// 真实 SMB 接入由 `Self::with_client`（未来 feature gated）落地。
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "smb-backend not enabled; rebuild with --features smb-backend",
        ))
    }

    /// 注入自定义 client。测试用 fake，未来 PR 注入 `RealSmbClient`。
    pub fn with_client(client: Arc<dyn SmbClient>) -> Self {
        Self { client }
    }

    /// Arc<dyn Backend> 工厂：方便 Registry / Index::with_backend 等单元注入。
    pub fn arc_with_client(client: Arc<dyn SmbClient>) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client))
    }
}

impl std::fmt::Debug for SmbBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmbBackend")
            .field("client", &self.client)
            .finish()
    }
}

impl Backend for SmbBackend {
    fn scheme(&self) -> &'static str {
        "smb"
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let target = build_target(loc)?;
        self.client.stat(&target).map_err(map_smb_error)
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        match self.metadata(loc) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        let target = match build_target(root) {
            Ok(t) => t,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let entries = match self.client.list(&target).map_err(map_smb_error) {
            Ok(v) => v,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        Box::new(entries.into_iter().map(Ok))
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = build_target(loc)?;
        let bytes = self.client.read(&target).map_err(map_smb_error)?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    fn open_write(
        &self,
        loc: &Location,
        mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>> {
        let target = build_target(loc)?;
        if mkparents {
            if let Some(parent_target) = parent_target(&target) {
                // 父目录创建失败（如已存在）由 client 自行决定是否当作错误；
                // 这里只透传，避免重复 IO Err 映射。
                let _ = self.client.mkdir(&parent_target);
            }
        }
        Ok(Box::new(SmbBufferedWriter {
            target,
            client: Arc::clone(&self.client),
            buffer: Vec::new(),
        }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let target = build_target(loc)?;
        self.client.unlink(&target).map_err(map_smb_error)
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let target = build_target(loc)?;
        self.client.mkdir(&target).map_err(map_smb_error)
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let bytes = {
            let target = build_target(loc)?;
            self.client.read(&target).map_err(map_smb_error)?
        };
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        mkparents: bool,
    ) -> io::Result<u64> {
        let src_target = build_target(src)?;
        let dst_target = build_target(dst)?;
        if mkparents {
            if let Some(p) = parent_target(&dst_target) {
                let _ = self.client.mkdir(&p);
            }
        }
        let bytes = self.client.read(&src_target).map_err(map_smb_error)?;
        self.client.write(&dst_target, &bytes).map_err(map_smb_error)
    }
}

/// SMB 写句柄：buffer 暂存 write 调用，`finish` 时一次性提交给 client。
/// 持 `Arc<dyn SmbClient>` 与 SmbBackend 共享 client 实例。
struct SmbBufferedWriter {
    target: SmbTarget,
    client: Arc<dyn SmbClient>,
    buffer: Vec<u8>,
}

impl std::fmt::Debug for SmbBufferedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmbBufferedWriter")
            .field("target", &self.target)
            .field("buffered_bytes", &self.buffer.len())
            .finish()
    }
}

impl io::Write for SmbBufferedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MediaWriter for SmbBufferedWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.client
            .write(&self.target, &self.buffer)
            .map(|_| ())
            .map_err(map_smb_error)
    }
}

/// 从 [`Location`] 解出 [`SmbTarget`]，并把 env 凭据合并进去。
/// 非 SMB scheme 返回 [`io::ErrorKind::InvalidInput`]。
fn build_target(loc: &Location) -> io::Result<SmbTarget> {
    let Location::Smb {
        user,
        host,
        port,
        share,
        path,
    } = loc
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("SmbBackend cannot handle scheme {:?}", loc.scheme()),
        ));
    };
    Ok(SmbTarget {
        user: user.clone(),
        host: host.clone(),
        port: *port,
        share: share.clone(),
        path: path.clone(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    })
}

/// 取目标在 share 内的父目录。若 path 已在 share 根（无父），返回 None
/// 让上层不去尝试 mkdir(share 根)。
fn parent_target(t: &SmbTarget) -> Option<SmbTarget> {
    let parent = t.path.parent()?;
    if parent.as_str().is_empty() {
        return None;
    }
    Some(SmbTarget {
        path: parent.to_path_buf(),
        ..t.clone()
    })
}

/// EACCES → PermissionDenied 等少数显式映射；其他 ErrorKind 透传。
/// 真实 smb crate 多用 [`io::Error::other`] 包错误码，前缀 "EACCES" 检测足够覆盖
/// 单测 + 简单远端场景；未来 PR 加 RealSmbClient 时按需扩展。
fn map_smb_error(e: io::Error) -> io::Error {
    if e.kind() == io::ErrorKind::Other && e.to_string().contains("EACCES") {
        return io::Error::new(io::ErrorKind::PermissionDenied, e.to_string());
    }
    e
}

#[cfg(test)]
#[path = "smb_tests.rs"]
mod tests;

#[cfg(feature = "smb-backend")]
#[path = "smb_real.rs"]
pub mod real;
