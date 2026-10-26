use std::collections::{HashMap, HashSet};
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::Mutex;
use tract_onnx::prelude::*;

use super::tract_dbnet::{RawDetector, TractDbnetDetector};
use crate::usecases::config::OcrConfig;
use crate::usecases::ocr::TextDetector;

pub struct FakeTextDetector {
    results: Mutex<HashMap<Utf8PathBuf, bool>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: bool,
}

impl FakeTextDetector {
    #[must_use]
    pub fn new(default: bool) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, has_text: bool) -> Self {
        self.results.lock().insert(path.into(), has_text);
        self
    }

    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeTextDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTextDetector")
            .field("default", &self.default)
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl TextDetector for FakeTextDetector {
    fn has_text(&self, path: &Utf8Path, _image_bytes: &[u8]) -> io::Result<bool> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeTextDetector: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .copied()
            .unwrap_or(self.default))
    }
}

struct TractFakeRaw {
    value: Option<f32>,
}

impl RawDetector for TractFakeRaw {
    fn run(&self, _input: Tensor) -> io::Result<Tensor> {
        let Some(value) = self.value else {
            return Err(io::Error::other("stub tract failure"));
        };
        Ok(
            tract_ndarray::Array4::from_shape_vec((1, 1, 1, 1), vec![value])
                .expect("stub tensor shape")
                .into_tensor(),
        )
    }
}

#[doc(hidden)]
#[must_use]
pub fn dbnet_with_output(cfg: OcrConfig, value: f32) -> Box<dyn TextDetector> {
    Box::new(TractDbnetDetector {
        cfg,
        raw: Mutex::new(Some(Box::new(TractFakeRaw { value: Some(value) }))),
    })
}

#[doc(hidden)]
#[must_use]
pub fn dbnet_with_error(cfg: OcrConfig) -> Box<dyn TextDetector> {
    Box::new(TractDbnetDetector {
        cfg,
        raw: Mutex::new(Some(Box::new(TractFakeRaw { value: None }))),
    })
}

#[cfg(test)]
#[path = "fake_tests.rs"]
mod tests;
