use std::io;

use parking_lot::Mutex;

use super::{LoadedState, RawEmbedder, TractEmbedClassifier, build_prototypes};
use crate::usecases::config::ClassifyConfig;

impl TractEmbedClassifier {
    pub(crate) fn with_raw(cfg: ClassifyConfig, raw: Box<dyn RawEmbedder>) -> io::Result<Self> {
        let prototypes = build_prototypes(raw.as_ref(), &cfg)?;
        Ok(Self {
            cfg,
            state: Mutex::new(Some(LoadedState { raw, prototypes })),
        })
    }
}
