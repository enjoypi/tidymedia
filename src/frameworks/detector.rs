use std::io;

use crate::adapters;
use crate::usecases::classify::DocumentClassifier;
use crate::usecases::config::try_config;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::ocr::TextDetector;

#[derive(Debug, Default)]
pub struct DefaultDetectorFactory;

impl DetectorFactory for DefaultDetectorFactory {
    fn build_text_detector(&self) -> io::Result<Box<dyn TextDetector>> {
        let cfg = try_config().map_err(std::io::Error::other)?;
        adapters::ocr::build_detector(&cfg.backend.ocr)
    }

    fn build_document_classifier(&self) -> io::Result<Box<dyn DocumentClassifier>> {
        let cfg = try_config().map_err(std::io::Error::other)?;
        adapters::classify::build_classifier(&cfg.backend.classify)
    }
}
