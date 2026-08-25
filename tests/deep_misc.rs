#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::manual_is_multiple_of,
    clippy::redundant_clone,
    clippy::single_char_pattern,
    clippy::unnecessary_trailing_comma
)]

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;
use tidymedia::{
    Backend, BackendFactory, CommandResult, Commands, DefaultDetectorFactory, Entry, FakeBackend,
    Location, MediaReader, MediaWriter, Metadata, Result, tidy_with,
};

// lib.rs re-export 观测口（`#[doc(hidden)]`）：
use tidymedia::{best_effort_remove_partial_dst, partial_move_error, under_prefix};

#[test]
fn under_prefix_matches_windows_backslash_rest() {
    assert!(under_prefix(r"C:\photos\img.jpg", r"C:\photos"));
    assert!(!under_prefix(r"C:\photos\img.jpg", r"C:\photoshare"));
}

#[test]
fn best_effort_remove_partial_dst_covers_err_and_ok() {
    let be = FakeBackend::new("local");
    let missing = Location::Local(Utf8PathBuf::from("out/group-000/BEST_x.jpg"));
    be.inject_error(
        missing.clone(),
        tidymedia::FakeOp::RemoveFile,
        io::ErrorKind::PermissionDenied,
    );
    best_effort_remove_partial_dst(&be, &missing);
    let present = Location::Local(Utf8PathBuf::from("out/group-000/BEST_y.jpg"));
    be.add_file(present.clone(), vec![1]);
    best_effort_remove_partial_dst(&be, &present);
}

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
    let be = Arc::new(PartialMoveRenameBackend::new());
    be.inner.add_dir(src_root.clone());
    be.inner.add_file(src_file.clone(), vec![0xAB; 4096]);
    let factory = AlwaysSameBackendFactory(Arc::clone(&be) as Arc<dyn Backend>);
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
fn strip_source_root_handles_missing_filename() {
    assert_eq!(tidymedia::strip_source_root("/", &[]), "/");
    assert_eq!(tidymedia::strip_source_root("/a/b.jpg", &[]), "b.jpg");
    assert_eq!(
        tidymedia::strip_source_root("/root/x.jpg", &["/root".to_string()]),
        "x.jpg"
    );
}
