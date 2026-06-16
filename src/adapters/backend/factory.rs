use std::sync::Arc;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::entities::uri::Location;

/// Backend 装配抽象：按 [`Location`] 构造对应的 [`Backend`] 句柄。
///
/// 生产路径走 [`DefaultBackendFactory`]：Local 直接给 [`LocalBackend`]，SMB / MTP
/// 在未启用对应 feature 时报 `Unsupported`。测试用 fake 实现注入
/// 覆盖跨 scheme 调度（见 `tests/lib_tidy.rs`）。
pub trait BackendFactory: Send + Sync {
    /// 根据 `loc` 的 scheme 构造并返回对应的 [`Backend`] 实现。
    ///
    /// # Errors
    ///
    /// 当对应 backend feature 未启用，或 backend 初始化（连接 / 认证）失败时返回 `Err`。
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>>;
}

/// 生产 [`BackendFactory`]：根据 Location.scheme 选 backend；当前仅 Local 真实可用，
/// SMB / MTP / ADB 真实适配器分别由对应 cargo feature 启用，未启用时返 `Unsupported`。
#[derive(Debug, Default)]
pub struct DefaultBackendFactory;

impl BackendFactory for DefaultBackendFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        match loc {
            Location::Local(_) => Ok(LocalBackend::arc()),
            Location::Smb { .. } => build_smb_backend(loc),
            Location::Mtp { .. } => build_mtp_backend(loc),
            Location::Adb { .. } => build_adb_backend(loc),
        }
    }
}

#[cfg(feature = "smb-backend")]
fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::smb::SmbBackend;
    use crate::adapters::backend::smb::SmbTarget;
    use crate::adapters::backend::smb::real::RealSmbClient;
    // 调度层保证只把 Location::Smb 路由到此处；let-else 仅在重构破坏不变量时触发。
    let Location::Smb {
        user,
        host,
        port,
        share,
        ..
    } = loc
    else {
        unreachable!("DefaultBackendFactory routes only Location::Smb here")
    };
    let target = SmbTarget {
        user: user.clone(),
        host: host.clone(),
        port: *port,
        share: share.clone(),
        path: camino::Utf8PathBuf::default(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    };
    let cfg = &crate::frameworks::config::config().backend.smb;
    let client =
        RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup).map_err(Error::Io)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(not(feature = "smb-backend"))]
fn build_smb_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "smb backend not enabled in this build; rebuild with --features smb-backend",
    )))
}

#[cfg(feature = "mtp-backend")]
fn build_mtp_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::mtp::real::RealMtpClient;
    // stub 期 RealMtpClient::new() 必 Err，? 自然传播。
    // 真实实现完成时改为 wrap 成 Backend 返回；当前 fallthrough 报 Unsupported，
    // 避免原 unreachable!() 在 stub 成为可用时变成运行期 panic。
    let _client = RealMtpClient::new()?;
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "mtp backend not enabled in this build; rebuild with --features mtp-backend",
    )))
}

#[cfg(not(feature = "mtp-backend"))]
fn build_mtp_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "mtp backend not enabled in this build; rebuild with --features mtp-backend",
    )))
}

#[cfg(feature = "adb-backend")]
fn build_adb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::adb::AdbBackend;
    use crate::adapters::backend::adb::real::RealAdbClient;
    let Location::Adb { serial, .. } = loc else {
        unreachable!("DefaultBackendFactory routes only Location::Adb here")
    };
    let cfg = &crate::frameworks::config::config().backend.adb;
    let client =
        RealAdbClient::new(serial.clone(), &cfg.server_host, cfg.server_port).map_err(Error::Io)?;
    Ok(AdbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(not(feature = "adb-backend"))]
fn build_adb_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "adb backend not enabled in this build; rebuild with --features adb-backend",
    )))
}
