use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::{TempDir, tempdir};

use super::*;
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;
use crate::usecases::report::CopyReport;

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn write_same_bucket_pair(dir: &Path) {
    let head = vec![7u8; 4096];
    for (name, tail) in [("a.bin", 1u8), ("b.bin", 2u8)] {
        let mut data = head.clone();
        data.push(tail);
        let p = dir.join(name);
        fs::write(&p, data).unwrap();
        filetime::set_file_mtime(
            &p,
            filetime::FileTime::from_unix_time(tc::FIXED_MEDIA_MTIME, 0),
        )
        .unwrap();
    }
}

fn run_with_mkdir_error(kind: ErrorKind) -> (TempDir, CopyReport) {
    let src = tempdir().unwrap();
    write_same_bucket_pair(src.path());
    let fake_out = Arc::new(FakeBackend::new("smb"));
    fake_out.add_dir(smb_loc("out"));
    fake_out.inject_error(smb_loc("out/2024"), Op::MkdirP, kind);
    let out_be: Arc<dyn Backend> = fake_out;
    let src_be: Arc<dyn Backend> = LocalBackend::arc();
    let src_loc = Location::Local(Utf8PathBuf::from(src.path().to_str().unwrap()));
    let report = copy(
        &[(src_loc, src_be)],
        (smb_loc("out"), out_be),
        false,
        false,
        true,
        Some("{year}"),
        None,
    )
    .unwrap();
    (src, report)
}

#[test]
fn output_write_denied_aborts_remaining_items() {
    let (_src, report) = run_with_mkdir_error(ErrorKind::PermissionDenied);
    assert_eq!(report.copied, 0);
    assert_eq!(report.failed, 2);
    assert_eq!(report.errors.len(), 2);
    let summary = &report.errors[1].message;
    assert!(summary.contains("aborted 1 remaining"));
    assert!(summary.contains("Controlled Folder Access"));
}

#[test]
fn non_denied_output_error_does_not_abort() {
    let (_src, report) = run_with_mkdir_error(ErrorKind::TimedOut);
    assert_eq!(report.copied, 0);
    assert_eq!(report.failed, 2);
    assert_eq!(report.errors.len(), 2);
    let aborted = report.errors.iter().any(|e| e.message.contains("aborted"));
    assert!(!aborted);
}
