use std::sync::Arc;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::Result;
use crate::entities::uri::Location;

use super::factory_real::build_smb_backend;

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
