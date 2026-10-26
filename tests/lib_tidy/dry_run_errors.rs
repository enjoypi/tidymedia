use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tidymedia::{
    Backend, CommandResult, Commands, DefaultDetectorFactory, Entry, FakeBackend, Location,
    MediaReader, MediaWriter, Metadata, tidy_with,
};

use super::{FakeBackendFactory, local};

struct ThirdReadFails {
    inner: FakeBackend,
    reads: AtomicUsize,
}

impl ThirdReadFails {
    fn new() -> Self {
        Self {
            inner: FakeBackend::new("local"),
            reads: AtomicUsize::new(0),
        }
    }
}

impl Backend for ThirdReadFails {
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
        if self.reads.fetch_add(1, Ordering::SeqCst) == 2 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected secure read failure",
            ));
        }
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
}

#[test]
fn dry_run_reports_secure_read_failure_without_writing_or_removing() {
    let root = local("/src");
    let file = local("/src/photo.bin");
    let backend = Arc::new(ThirdReadFails::new());
    backend.inner.add_dir(root.clone());
    backend.inner.add_file(file, b"payload".to_vec());
    let mut factory = FakeBackendFactory::new();
    factory.insert("local", backend.clone());
    let result = tidy_with(
        &factory,
        &DefaultDetectorFactory,
        Commands::Copy {
            dry_run: true,
            include_non_media: true,
            sources: vec![root],
            output: local("/out"),
            archive_template: None,
            report: None,
        },
    )
    .unwrap();
    let CommandResult::Copy(report) = result else {
        panic!("expected copy report");
    };
    assert_eq!(
        report.failed,
        1,
        "{report:?} reads={}",
        backend.reads.load(Ordering::SeqCst)
    );
    assert!(
        report.errors[0]
            .message
            .contains("injected secure read failure")
    );
}
