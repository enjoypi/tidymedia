use std::sync::Arc;

use camino::Utf8PathBuf;

use crate::adapters::backend::local::LocalBackend;
use crate::adapters::backend::smb::real::RealSmbClient;
use crate::adapters::backend::smb::{SmbBackend, SmbTarget};
use crate::entities::backend::Backend;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;

#[derive(Debug, Default)]
pub struct DefaultBackendFactory;

impl BackendFactory for DefaultBackendFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        match loc {
            Location::Local(_) => Ok(LocalBackend::arc()),
            Location::Smb { .. } => build_smb_backend(loc),
        }
    }
}

pub fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    let Location::Smb {
        user,
        host,
        port,
        share,
        ..
    } = loc
    else {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "SMB backend requires smb location, got scheme {:?}",
                loc.scheme()
            ),
        )));
    };
    let target = SmbTarget {
        user: user.clone(),
        host: host.clone(),
        port: *port,
        share: share.clone(),
        path: Utf8PathBuf::default(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    };
    let cfg = &crate::usecases::config::checked_config()?.backend.smb;
    let client = RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup, cfg.timeout_secs)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
