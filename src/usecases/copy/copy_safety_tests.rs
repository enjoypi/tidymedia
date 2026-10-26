use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashSet;
use tempfile::tempdir;

use super::ops::{do_copy as do_copy_raw, refuse_self_duplicate};
use super::png_fixture::write_unique_png;
use super::*;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::Index;
use crate::entities::uri::Location;

const MONTH_TMPL: &str = "{year}/{month}";

fn local_loc(p: &Path) -> Location {
    Location::Local(Utf8PathBuf::from(p.to_str().unwrap()))
}

fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
    (local_loc(p), LocalBackend::arc())
}

fn media_info(p: &Path) -> Info {
    let mut info = Info::from(p.to_str().unwrap()).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    info
}

fn contents_under(dir: &Path) -> BTreeSet<Vec<u8>> {
    let mut set = BTreeSet::new();
    for e in fs::read_dir(dir).unwrap() {
        set.insert(fs::read(e.unwrap().path()).unwrap());
    }
    set
}

#[test]
fn parallel_same_name_sources_never_overwrite_each_other() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let mut want = BTreeSet::new();
    for i in 0..11u64 {
        let sub = src.path().join(format!("cam{i:02}"));
        fs::create_dir(&sub).unwrap();
        want.insert(fs::read(write_unique_png(&sub, "IMG_0001.png", i)).unwrap());
    }
    let report = copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        true,
        false,
        Some(MONTH_TMPL),
        None,
    )
    .unwrap();
    assert_eq!((report.copied, report.failed), (11, 0));
    assert_eq!(contents_under(&out.path().join("2024").join("01")), want);
}

#[test]
fn case_variant_of_existing_target_gets_suffix() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let month = out.path().join("2024").join("01");
    fs::create_dir_all(&month).unwrap();
    let old = fs::read(write_unique_png(&month, "IMG_0001.PNG", 1)).unwrap();
    let new = fs::read(write_unique_png(src.path(), "img_0001.png", 2)).unwrap();
    let report = copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        true,
        false,
        Some(MONTH_TMPL),
        None,
    )
    .unwrap();
    assert_eq!((report.copied, report.failed), (1, 0));
    assert_eq!(fs::read(month.join("IMG_0001.PNG")).unwrap(), old);
    assert_eq!(contents_under(&month), BTreeSet::from([old, new]));
}

#[test]
fn unindexed_existing_target_is_kept_and_next_name_used() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let month = out.path().join("2024").join("01");
    fs::create_dir_all(&month).unwrap();
    let old = fs::read(write_unique_png(&month, "photo.png", 1)).unwrap();
    let p = write_unique_png(src.path(), "photo.png", 2);
    let new = fs::read(&p).unwrap();
    let info = media_info(&p);
    let opts = move_opts();
    let copied = do_copy_raw(
        &info,
        &local_loc(out.path()),
        &LocalBackend::arc(),
        &Index::new(),
        &DashSet::new(),
        true,
        &opts,
    )
    .unwrap();
    assert!(copied);
    assert_eq!(fs::read(month.join("photo.png")).unwrap(), old);
    assert_eq!(fs::read(month.join("photo_1.png")).unwrap(), new);
    assert!(!p.exists());
}

fn move_opts() -> CopyOpts<'static> {
    CopyOpts {
        dry_run: false,
        remove: true,
        include_non_media: false,
        doc_only: false,
        template: MONTH_TMPL,
    }
}

#[test]
fn move_dry_run_does_not_delete_duplicate_source() {
    let src = tempdir().unwrap();
    let p = write_unique_png(src.path(), "duplicate.png", 7);
    let info = media_info(&p);
    let idx = Index::new();
    idx.add(media_info(&p));
    let opts = CopyOpts {
        dry_run: true,
        ..move_opts()
    };
    let copied = do_copy_raw(
        &info,
        &local_loc(src.path()),
        &LocalBackend::arc(),
        &idx,
        &DashSet::new(),
        true,
        &opts,
    )
    .unwrap();
    assert!(!copied);
    assert!(p.exists());
}

#[test]
fn move_keeps_non_media_source_that_duplicates_output() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    fs::write(src.path().join("notes.txt"), b"same text").unwrap();
    fs::write(out.path().join("notes.txt"), b"same text").unwrap();
    let report = copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        true,
        false,
        Some(MONTH_TMPL),
        None,
    )
    .unwrap();
    assert_eq!(report.copied, 0);
    assert!(src.path().join("notes.txt").exists());
}

#[test]
fn move_refuses_to_delete_source_that_is_its_own_duplicate() {
    let src = tempdir().unwrap();
    let p = write_unique_png(src.path(), "self.png", 7);
    let info = media_info(&p);
    let idx = Index::new();
    idx.add(media_info(&p));
    let err = do_copy_raw(
        &info,
        &local_loc(src.path()),
        &LocalBackend::arc(),
        &idx,
        &DashSet::new(),
        true,
        &move_opts(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("refusing to remove"));
    assert!(p.exists());
}

#[test]
fn refuse_self_duplicate_allows_distinct_and_missing_and_remote() {
    let dir = tempdir().unwrap();
    let a = write_unique_png(dir.path(), "a.png", 1);
    let b = write_unique_png(dir.path(), "b.png", 2);
    let ub = Utf8PathBuf::from(b.to_str().unwrap());
    let missing = Utf8PathBuf::from(dir.path().join("gone.png").to_str().unwrap());
    let remote = Location::parse("smb://nas/share/a.png").unwrap();
    assert!(refuse_self_duplicate(&local_loc(&a), &ub).is_ok());
    assert!(refuse_self_duplicate(&local_loc(&a), &missing).is_ok());
    assert!(refuse_self_duplicate(&remote, &ub).is_ok());
}
