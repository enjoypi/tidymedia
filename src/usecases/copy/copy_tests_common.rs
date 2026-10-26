use std::fs;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashSet;

use super::ops::do_copy;
use super::{CopyOpts, Info, Source, copy_with_sidecar};
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;
use crate::usecases::report::{CopyReport, ReportSink};

pub(super) const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

pub(super) fn copy(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
) -> common::Result<CopyReport> {
    copy_with_sidecar(
        sources,
        output,
        dry_run,
        remove,
        include_non_media,
        false,
        archive_template,
        report_sink,
        None,
        None,
    )
}

pub(super) fn do_copy_with_default_cache(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &crate::entities::file_index::Index,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let mc: DashSet<Location> = DashSet::new();
    do_copy(
        src,
        output_dir,
        output_backend,
        output_index,
        &mc,
        false,
        opts,
    )
}

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
