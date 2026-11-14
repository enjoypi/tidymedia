use std::path::Path;

use tempfile::tempdir;

use super::ops::{count_type_filtered, type_allowed};
use super::png_fixture::write_unique_png;
use super::*;
use crate::entities::exif::Exif;
use crate::entities::file_index::Index;

fn opts(include_non_media: bool, doc_only: bool) -> CopyOpts<'static> {
    CopyOpts {
        dry_run: true,
        remove: false,
        include_non_media,
        doc_only,
        template: "{year}/{month}",
    }
}

fn info_with_mime(dir: &Path, name: &str, unique: u64, mime: &str) -> Info {
    let p = write_unique_png(dir, name, unique);
    let mut info = Info::from(p.to_str().unwrap()).unwrap();
    info.set_exif(Exif::with_mime(mime));
    info
}

#[test]
fn type_allowed_follows_three_modes() {
    let dir = tempdir().unwrap();
    let media = info_with_mime(dir.path(), "a.png", 1, "image/png");
    let text = info_with_mime(dir.path(), "b.txt", 2, "text/plain");
    let pdf = info_with_mime(dir.path(), "c.pdf", 3, "application/pdf");
    assert!(type_allowed(&media, false, false));
    assert!(!type_allowed(&text, false, false));
    assert!(type_allowed(&text, true, false));
    assert!(type_allowed(&pdf, false, true));
    assert!(!type_allowed(&media, false, true));
}

#[test]
fn count_type_filtered_counts_rejected_files() {
    let dir = tempdir().unwrap();
    let idx = Index::new();
    idx.add(info_with_mime(dir.path(), "a.png", 1, "image/png"));
    idx.add(info_with_mime(dir.path(), "b.txt", 2, "text/plain"));
    idx.add(info_with_mime(dir.path(), "c.txt", 3, "text/plain"));
    assert_eq!(count_type_filtered(&idx, &opts(false, false)), 2);
    assert_eq!(count_type_filtered(&idx, &opts(true, false)), 0);
}

fn copy_report(
    src: &Path,
    out: &Path,
    include_non_media: bool,
) -> crate::usecases::report::CopyReport {
    let be = crate::adapters::backend::local::LocalBackend::arc;
    let loc = |p: &Path| {
        crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(p.to_str().unwrap()))
    };
    copy(
        &[(loc(src), be())],
        (loc(out), be()),
        true,
        false,
        include_non_media,
        Some("{year}/{month}"),
        None,
    )
    .unwrap()
}

#[test]
fn copy_skips_listed_extensions_but_counts_them_as_scanned() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    write_unique_png(src.path(), "a.png", 1);
    std::fs::write(src.path().join("app.js"), b"console.log(1)").unwrap();
    let report = copy_report(src.path(), out.path(), false);
    assert_eq!((report.scanned, report.copied, report.ignored), (2, 1, 0));
}

#[test]
fn dry_run_reads_only_the_fast_hash_window_without_peers() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let png = write_unique_png(src.path(), "a.png", 1);
    let size = std::fs::metadata(png).unwrap().len();
    let report = copy_report(src.path(), out.path(), false);
    assert!(size > 0);
    assert_eq!(report.bytes_read, size.min(4096));
}

#[test]
fn include_non_media_disables_extension_skip() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    std::fs::write(src.path().join("app.js"), b"console.log(1)").unwrap();
    let report = copy_report(src.path(), out.path(), true);
    assert_eq!((report.scanned, report.copied), (1, 1));
}

fn parity_counts(dry_run: bool) -> (usize, usize, usize) {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let head = vec![7_u8; 4096];
    let with_tail = |tail: u8| [head.as_slice(), &[tail; 904]].concat();
    std::fs::write(src.path().join("a.bin"), with_tail(1)).unwrap();
    std::fs::write(src.path().join("b.bin"), with_tail(1)).unwrap();
    std::fs::write(src.path().join("c.bin"), with_tail(2)).unwrap();
    let be = crate::adapters::backend::local::LocalBackend::arc;
    let loc = |p: &Path| {
        crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(p.to_str().unwrap()))
    };
    let report = copy(
        &[(loc(src.path()), be())],
        (loc(out.path()), be()),
        dry_run,
        false,
        true,
        Some("{year}/{month}"),
        None,
    )
    .unwrap();
    (report.copied, report.ignored, report.failed)
}

#[test]
fn dry_run_matches_real_run_for_same_head_same_size_files() {
    assert_eq!(parity_counts(true), (2, 1, 0));
    assert_eq!(parity_counts(false), (2, 1, 0));
}
