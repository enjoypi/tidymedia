use std::sync::Arc;

use crate::entities::backend::local::LocalBackend;
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

fn unsupported_backend(loc: &Location, feature: &str) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "{} backend not enabled in this build; rebuild with --features {}",
            loc.scheme(),
            feature,
        ),
    ))
}

#[cfg(feature = "smb-backend")]
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::smb::SmbBackend;
    use crate::entities::backend::smb::real::RealSmbClient;
    use crate::entities::backend::smb::SmbTarget;
    let Location::Smb { user, host, port, share, .. } = loc else {
        unreachable!("DefaultBackendFactory routes only Location::Smb here")
    };
    let target = SmbTarget {
        user: user.clone(),
        host: host.clone(),
        port: *port,
        share: share.clone(),
        path: Default::default(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    };
    let cfg = &crate::frameworks::config::config().backend.smb;
    let client = RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup)
        .map_err(Error::Io)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(not(feature = "smb-backend"))]
fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(unsupported_backend(loc, "smb-backend"))
}

#[cfg(feature = "mtp-backend")]
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_mtp_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::entities::backend::mtp::real::RealMtpClient;
    let _ = loc;
    let _ = RealMtpClient::new()?;
    unreachable!("RealMtpClient::new always returns Err in the stub phase");
}

#[cfg(not(feature = "mtp-backend"))]
fn build_mtp_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(unsupported_backend(loc, "mtp-backend"))
}

#[cfg(feature = "adb-backend")]
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_adb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::adb::AdbBackend;
    use crate::entities::backend::adb::real::RealAdbClient;
    let Location::Adb { serial, .. } = loc else {
        unreachable!("DefaultBackendFactory routes only Location::Adb here")
    };
    let cfg = &crate::frameworks::config::config().backend.adb;
    let client =
        RealAdbClient::new(serial.clone(), &cfg.server_host, cfg.server_port).map_err(Error::Io)?;
    Ok(AdbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(not(feature = "adb-backend"))]
fn build_adb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(unsupported_backend(loc, "adb-backend"))
}
