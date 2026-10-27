use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::{CommandResult, tidy_with};
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::cli::Commands;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::backend::{
    Backend, Entry, MediaReader, MediaWriter, Metadata, partial_move_error,
};
use crate::entities::common::Result;
use crate::entities::uri::Location;
use crate::frameworks::detector::DefaultDetectorFactory;

struct PartialMoveRenameBackend {
    inner: FakeBackend,
}

impl PartialMoveRenameBackend {
    fn new() -> Self {
        Self {
            inner: FakeBackend::new("local"),
        }
    }
}

impl Backend for PartialMoveRenameBackend {
    fn scheme(&self) -> &'static str {
        self.inner.scheme()
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        self.inner.metadata(loc)
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        self.inner.exists(loc)
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        self.inner.walk(root)
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        self.inner.open_read(loc)
    }

    fn open_write(&self, loc: &Location, mkparents: bool) -> io::Result<Box<dyn MediaWriter>> {
        self.inner.open_write(loc, mkparents)
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        self.inner.remove_file(loc)
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        self.inner.mkdir_p(loc)
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        self.inner.read_to_string(loc)
    }

    fn copy_file(&self, src: &Location, dst: &Location, mkparents: bool) -> io::Result<u64> {
        self.inner.copy_file(src, dst, mkparents)
    }

    fn supports_native_rename_to(&self, _other: &dyn Backend) -> bool {
        true
    }

    fn rename(&self, from: &Location, to: &Location, _mkparents: bool) -> io::Result<()> {
        Err(partial_move_error(
            io::ErrorKind::PermissionDenied,
            format!(
                "copy: copied {} -> {} but cannot remove source",
                from.display(),
                to.display()
            ),
        ))
    }
}

struct AlwaysSameBackendFactory(Arc<dyn Backend>);

impl BackendFactory for AlwaysSameBackendFactory {
    fn for_location(&self, _loc: &Location) -> Result<Arc<dyn Backend>> {
        Ok(Arc::clone(&self.0))
    }
}

#[test]
fn move_with_partial_move_rename_registers_dst_index_and_fails() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_string();
    let src_root = Location::Local(Utf8PathBuf::from(format!("{root}/src")));
    let src_file = Location::Local(Utf8PathBuf::from(format!("{root}/src/photo.jpg")));
    let be: Arc<PartialMoveRenameBackend> = Arc::new(PartialMoveRenameBackend::new());
    be.inner.add_dir(src_root.clone());
    be.inner.add_file(src_file, vec![0xAB; 4096]);
    let factory = AlwaysSameBackendFactory(be);
    let result = tidy_with(
        &factory,
        &DefaultDetectorFactory,
        Commands::Move {
            dry_run: false,
            include_non_media: true,
            sources: vec![src_root],
            output: Location::Local(Utf8PathBuf::from(format!("{root}/out"))),
            archive_template: None,
            report: None,
        },
    )
    .expect("move returns Ok with per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    assert_eq!(report.failed, 1);
    assert_eq!(report.copied, 0);
}

#[test]
fn tidy_with_prechecks_config_before_dispatch() {
    crate::usecases::config::install_loader(|| Err("cfg broken".to_string()));
    let dir = tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_string();
    let result = tidy_with(
        &AlwaysSameBackendFactory(Arc::new(FakeBackend::new("local"))),
        &DefaultDetectorFactory,
        Commands::Move {
            dry_run: true,
            include_non_media: false,
            sources: vec![Location::Local(Utf8PathBuf::from(format!("{root}/src")))],
            output: Location::Local(Utf8PathBuf::from(format!("{root}/out"))),
            archive_template: None,
            report: None,
        },
    );
    let err = result.expect_err("precheck must fail");
    assert!(format!("{err}").contains("cfg broken"), "got: {err}");
}

struct LocalOnlyFactory;

impl BackendFactory for LocalOnlyFactory {
    fn for_location(&self, _loc: &Location) -> Result<Arc<dyn Backend>> {
        Ok(crate::adapters::backend::local::LocalBackend::arc())
    }
}

fn local_loc_str(p: &std::path::Path) -> Location {
    Location::Local(Utf8PathBuf::from(p.to_str().unwrap()))
}

#[test]
fn find_report_write_failure_exits_nonzero() {
    let src = tempdir().unwrap();
    let result = tidy_with(
        &LocalOnlyFactory,
        &DefaultDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![local_loc_str(src.path())],
            output: None,
            report: Some("/nonexistent_dir_xyz/find_report.json".to_string()),
        },
    );
    let err = result.expect_err("report write failure must propagate");
    assert!(
        err.to_string().contains("nonexistent_dir_xyz"),
        "got: {err}"
    );
}

#[test]
fn verify_report_write_failure_exits_nonzero() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let result = tidy_with(
        &LocalOnlyFactory,
        &DefaultDetectorFactory,
        Commands::Verify {
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            include_non_media: false,
            exif_tsv: None,
            phash_max: None,
            report: Some("/nonexistent_dir_xyz/verify_report.json".to_string()),
        },
    );
    let err = result.expect_err("report write failure must propagate");
    assert!(
        err.to_string().contains("nonexistent_dir_xyz"),
        "got: {err}"
    );
}

#[derive(Debug)]
struct StubTextDetector;

impl crate::usecases::ocr::TextDetector for StubTextDetector {
    fn has_text(&self, _path: &camino::Utf8Path, _image_bytes: &[u8]) -> io::Result<bool> {
        Ok(false)
    }
}

#[derive(Debug)]
struct StubClassifier;

impl crate::usecases::classify::DocumentClassifier for StubClassifier {
    fn classify(
        &self,
        _path: &camino::Utf8Path,
        _text: &str,
    ) -> io::Result<crate::usecases::classify::Classification> {
        unreachable!("move-text-shot dispatch 不构造文档分类器")
    }
}

struct StubDetectorFactory;

impl crate::usecases::detector::DetectorFactory for StubDetectorFactory {
    fn build_text_detector(&self) -> io::Result<Box<dyn crate::usecases::ocr::TextDetector>> {
        Ok(Box::new(StubTextDetector))
    }

    fn build_document_classifier(
        &self,
    ) -> io::Result<Box<dyn crate::usecases::classify::DocumentClassifier>> {
        Ok(Box::new(StubClassifier))
    }
}

#[test]
fn move_text_shot_report_write_failure_exits_nonzero() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let result = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::MoveTextShot {
            dry_run: false,
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            report: Some("/nonexistent_dir_xyz/mts_report.json".to_string()),
        },
    );
    let err = result.expect_err("report write failure must propagate");
    assert!(
        err.to_string().contains("nonexistent_dir_xyz"),
        "got: {err}"
    );
}

include!("dispatch_more_tests.rs");
