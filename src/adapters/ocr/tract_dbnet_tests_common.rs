use parking_lot::Mutex;

use super::{RawDetector, TractDbnetDetector};
use crate::usecases::config::OcrConfig;

impl TractDbnetDetector {
    pub(crate) fn with_raw(cfg: OcrConfig, raw: Box<dyn RawDetector>) -> Self {
        Self {
            cfg,
            raw: Mutex::new(Some(raw)),
        }
    }
}
