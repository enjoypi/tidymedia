use std::io;

use crate::adapters;
use crate::usecases::classify::DocumentClassifier;
use crate::usecases::config::config;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::ocr::TextDetector;

#[derive(Debug, Default)]
pub struct DefaultDetectorFactory;

impl DetectorFactory for DefaultDetectorFactory {
    fn build_text_detector(&self) -> io::Result<Box<dyn TextDetector>> {
        adapters::ocr::build_detector(&config().backend.ocr)
    }

    fn build_document_classifier(&self) -> io::Result<Box<dyn DocumentClassifier>> {
        adapters::classify::build_classifier(&config().backend.classify)
    }
}
