use std::sync::Arc;

use super::Backend;
use crate::entities::common::Result;
use crate::entities::uri::Location;

pub trait BackendFactory: Send + Sync {
    #[expect(clippy::missing_errors_doc, reason = "backend 错误经统一 Result 传播")]
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>>;
}
