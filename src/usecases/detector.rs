use std::io;

use crate::usecases::classify::DocumentClassifier;
use crate::usecases::ocr::TextDetector;

pub trait DetectorFactory: Send + Sync {
    #[expect(clippy::missing_errors_doc)]
    fn build_text_detector(&self) -> io::Result<Box<dyn TextDetector>>;

    #[expect(clippy::missing_errors_doc)]
    fn build_document_classifier(&self) -> io::Result<Box<dyn DocumentClassifier>>;
}
