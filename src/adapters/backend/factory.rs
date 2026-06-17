use std::sync::Arc;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::backend::factory::BackendFactory;
#[cfg(not(all(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
)))]
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::entities::uri::Location;

#[cfg(not(all(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
)))]
use super::remote::unsupported_backend;

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
use super::factory_real::build_smb_backend;

#[cfg(not(feature = "smb-backend"))]
fn build_smb_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(unsupported_backend("smb-backend")))
}

#[cfg(feature = "mtp-backend")]
use super::factory_real::build_mtp_backend;

#[cfg(not(feature = "mtp-backend"))]
fn build_mtp_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(unsupported_backend("mtp-backend")))
}

#[cfg(feature = "adb-backend")]
use super::factory_real::build_adb_backend;

#[cfg(not(feature = "adb-backend"))]
fn build_adb_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(unsupported_backend("adb-backend")))
}
