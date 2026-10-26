use std::sync::Arc;

use crate::entities::backend::Backend;
use crate::entities::common::Result;
use crate::entities::uri::Location;

pub(super) fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::smb::SmbBackend;
    use crate::adapters::backend::smb::SmbTarget;
    use crate::adapters::backend::smb::real::RealSmbClient;

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
    let cfg = &crate::usecases::config::config().backend.smb;
    let client = RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup, cfg.timeout_secs)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}
