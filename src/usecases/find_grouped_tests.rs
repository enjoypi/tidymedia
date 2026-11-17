use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::escape_py_string;
use super::find_duplicates;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::uri::Location;
use crate::usecases::memory::with_retained;
use crate::usecases::report::FindReport;

fn seed(dir: &Path) {
    for i in 0..8_u8 {
        let body = vec![i; 300 + usize::from(i) * 13];
        fs::write(dir.join(format!("a{i}.bin")), &body).unwrap();
        if i % 2 == 0 {
            fs::write(dir.join(format!("b{i}.bin")), &body).unwrap();
        }
    }
    fs::write(dir.join("empty.bin"), b"").unwrap();
}

fn run(capacity: u64, secure: bool) -> (FindReport, String) {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let source = (
        Location::Local(Utf8PathBuf::from(dir.path().to_str().unwrap())),
        LocalBackend::arc(),
    );
    let mut script = Vec::new();
    let report = with_retained(capacity, || {
        find_duplicates(secure, &[source], None, &mut script).unwrap()
    });
    let root = dir.path().to_str().unwrap().to_owned();
    (
        report,
        String::from_utf8(script)
            .unwrap()
            .replace(&escape_py_string(&root), "")
            .replace(&root, ""),
    )
}

fn shape(report: &FindReport) -> (usize, Vec<(u64, usize)>) {
    let mut groups = Vec::new();
    report
        .groups
        .for_each(&mut |g| {
            groups.push((g.size, g.paths.len()));
            Ok(())
        })
        .unwrap();
    (report.scanned, groups)
}

#[test]
fn grouped_find_matches_single_pass() {
    for secure in [true, false] {
        let (single, single_script) = run(1 << 30, secure);
        let (grouped, grouped_script) = run(12_000, secure);
        assert_eq!(shape(&grouped), shape(&single));
        assert_eq!(grouped_script, single_script);
        assert_eq!(shape(&single).1.len(), 4);
        assert!(grouped.groups.is_spilled());
        assert_eq!(single.scanned, 13);
    }
}

#[test]
fn grouped_find_stops_when_one_bucket_exceeds_budget() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let source = (
        Location::Local(Utf8PathBuf::from(dir.path().to_str().unwrap())),
        LocalBackend::arc(),
    );
    let err = with_retained(1_000, || {
        find_duplicates(true, &[source], None, &mut Vec::new()).unwrap_err()
    });
    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), std::io::ErrorKind::OutOfMemory);
}
