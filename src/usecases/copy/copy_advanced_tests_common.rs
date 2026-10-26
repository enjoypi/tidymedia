use std::fs;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::{CopyOpts, Info};
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

pub(super) const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

fn utf8(path: &Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.to_str().unwrap())
}

pub(super) fn local_loc(path: &Path) -> Location {
    Location::Local(utf8(path))
}

pub(super) fn local_arc() -> Arc<dyn Backend> {
    LocalBackend::arc()
}

pub(super) fn local_source(path: &Path) -> (Location, Arc<dyn Backend>) {
    (local_loc(path), local_arc())
}

pub(super) fn make_media_info(dir: &Path, name: &str) -> Info {
    let png = tc::copy_png_to(dir, name).unwrap();
    let mut info = Info::from(png.to_str().unwrap()).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    info
}

pub(super) fn default_opts(template: &str) -> CopyOpts<'_> {
    CopyOpts {
        dry_run: false,
        remove: false,
        include_non_media: false,
        doc_only: false,
        template,
    }
}

pub(super) fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}
