use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::super::tests_common::copy;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;
use crate::usecases::memory::with_retained;
use crate::usecases::report::CopyReport;

const ROOMY: u64 = 1 << 30;
const TIGHT: u64 = 16_000;

fn put(dir: &Path, name: &str, data: &[u8]) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, data).unwrap();
    filetime::set_file_mtime(
        &path,
        filetime::FileTime::from_unix_time(tc::FIXED_MEDIA_MTIME, 0),
    )
    .unwrap();
}

fn seed(src: &Path, out: &Path) {
    for i in 0..10_u8 {
        put(
            src,
            &format!("f{i}.bin"),
            &vec![i; 100 + usize::from(i) * 37],
        );
    }
    put(src, "sub/f3.bin", &vec![3; 211]);
    put(src, "dup.bin", &vec![42; 500]);
    put(src, "sub/dup.bin", &vec![42; 500]);
    put(out, "old/f5.bin", &vec![5; 285]);
    put(out, "old/other.bin", &vec![9; 777]);
}

fn loc(path: &Path) -> Location {
    Location::Local(Utf8PathBuf::from(path.to_str().unwrap()))
}

fn listing(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                files.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

fn run(capacity: u64, dry_run: bool) -> (CopyReport, Vec<(String, Vec<u8>)>) {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let report = with_retained(capacity, || {
        copy(
            &[(loc(src.path()), LocalBackend::arc())],
            (loc(out.path()), LocalBackend::arc()),
            dry_run,
            false,
            true,
            Some("{year}/{month}"),
            None,
        )
        .unwrap()
    });
    (report, listing(out.path()))
}

fn counts(report: &CopyReport) -> (usize, usize, usize, usize, u64) {
    (
        report.scanned,
        report.copied,
        report.ignored,
        report.failed,
        report.skipped_empty,
    )
}

#[test]
fn grouped_copy_matches_single_pass_result() {
    let (single, single_tree) = run(ROOMY, false);
    let (grouped, grouped_tree) = run(TIGHT, false);
    assert_eq!(counts(&grouped), counts(&single));
    assert_eq!(grouped_tree, single_tree);
    assert_eq!((single.copied, single.ignored), (10, 3));
}

#[test]
fn grouped_dry_run_matches_single_pass_dry_run() {
    let (single, _) = run(ROOMY, true);
    let (grouped, tree) = run(TIGHT, true);
    assert_eq!(counts(&grouped), counts(&single));
    assert_eq!(tree.len(), 2);
}

#[test]
fn budget_smaller_than_one_entry_stops_before_any_write() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let before = listing(out.path());
    let err = with_retained(1_000, || {
        copy(
            &[(loc(src.path()), LocalBackend::arc())],
            (loc(out.path()), LocalBackend::arc()),
            false,
            true,
            true,
            Some("{year}/{month}"),
            None,
        )
        .unwrap_err()
    });
    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), std::io::ErrorKind::OutOfMemory);
    assert_eq!(listing(out.path()), before);
    assert_eq!(listing(src.path()).len(), 13);
}

fn grouped_error(capacity: u64, output: &Path, src: &Path) -> std::io::Error {
    let err = with_retained(capacity, || {
        copy(
            &[(loc(src), LocalBackend::arc())],
            (loc(output), LocalBackend::arc()),
            false,
            false,
            true,
            Some("{year}/{month}"),
            None,
        )
        .unwrap_err()
    });
    let crate::entities::common::Error::Io(io_err) = err;
    io_err
}

fn flood(dry_run: bool, capacity: u64) -> (CopyReport, Vec<(String, Vec<u8>)>) {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    for i in 0..6 {
        put(src.path(), &format!("d{i}/same.bin"), &[7; 300]);
    }
    put(src.path(), "other.bin", &[8; 300]);
    let report = with_retained(capacity, || {
        copy(
            &[(loc(src.path()), LocalBackend::arc())],
            (loc(out.path()), LocalBackend::arc()),
            dry_run,
            false,
            true,
            Some("{year}/{month}"),
            None,
        )
        .unwrap()
    });
    (report, listing(out.path()))
}

#[test]
fn identical_flood_is_chunked_and_matches_single_pass() {
    for dry_run in [false, true] {
        let (single, single_tree) = flood(dry_run, ROOMY);
        let (chunked, chunked_tree) = flood(dry_run, 9_000);
        assert_eq!(counts(&chunked), counts(&single));
        assert_eq!(chunked_tree, single_tree);
        assert_eq!((single.copied, single.ignored), (2, 5));
    }
}

#[test]
fn grouped_copy_propagates_output_mkdir_failure() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    seed(src.path(), out.path());
    let blocker = out.path().join("blocker");
    fs::write(&blocker, b"x").unwrap();
    let err = grouped_error(TIGHT, &blocker, src.path());
    assert_ne!(err.kind(), std::io::ErrorKind::OutOfMemory);
}
