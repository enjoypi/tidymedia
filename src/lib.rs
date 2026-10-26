mod adapters;
mod entities;
mod frameworks;
mod usecases;

pub use adapters::backend::local::LocalBackend;
pub use adapters::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use adapters::cli::{Cli, Commands, run_cli};
pub use adapters::dispatch::{CommandResult, tidy, tidy_with};
pub use frameworks::backend::DefaultBackendFactory;
pub use usecases::config::{CategoryDef, ClassifyConfig, OcrConfig, VerifyConfig};
pub use usecases::move_text_shot::MoveTextShotReport;
pub use usecases::verify::{VerifyEntry, VerifyReport};

pub use entities::backend::factory::BackendFactory;
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

pub use frameworks::detector::DefaultDetectorFactory;
pub use usecases::detector::DetectorFactory;

pub use adapters::sidecar;

#[doc(hidden)]
pub use adapters::backend::fake::{FakeBackend, Op as FakeOp};

pub use adapters::ocr::build_detector;
#[doc(hidden)]
pub use adapters::ocr::fake::{FakeTextDetector, dbnet_with_error, dbnet_with_output};
pub use usecases::ocr::TextDetector;

pub use adapters::classify::build_classifier;
#[doc(hidden)]
pub use adapters::classify::fake::FakeDocumentClassifier;
pub use usecases::classify::{Classification, DocumentClassifier};

pub fn install_config_loader() {
    frameworks::config::install_global_loader();
}

#[cfg(test)]
#[path = "../tests/support/tracing_fixture_tests.rs"]
mod tracing_fixture;
