use std::sync::Arc;

use super::test_common::*;
use super::*;
use crate::adapters::backend::fake::FakeBackend;
use crate::entities::backend::Backend;

#[test]
fn analyze_image_records_failure_on_read_all_err() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let loc = smb_loc("/src/x.png");
    fake.add_file(loc.clone(), b"whatever".to_vec());
    fake.inject_reader_error(loc.clone(), io::ErrorKind::TimedOut);
    let item = ScannedFile {
        src_loc: loc,
        src_backend: fake as Arc<dyn Backend>,
        source_root: smb_loc("/src"),
        hash: 0,
        sharpness: 100.0,
    };
    let mut failures = Vec::new();
    let out = call_analyze_image(&item, &mut failures);
    assert!(out.is_none());
    assert_eq!(failures.len(), 1);
    assert!(failures[0].1.to_string().contains("timed out"));
}

#[test]
fn analyze_image_records_failure_on_decode_err() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let loc = smb_loc("/src/y.png");
    fake.add_file(loc.clone(), b"not-image-bytes".to_vec());
    let item = ScannedFile {
        src_loc: loc,
        src_backend: fake as Arc<dyn Backend>,
        source_root: smb_loc("/src"),
        hash: 0,
        sharpness: 100.0,
    };
    let mut failures = Vec::new();
    let out = call_analyze_image(&item, &mut failures);
    assert!(out.is_none());
    assert_eq!(failures.len(), 1);
    assert!(failures[0].1.to_string().contains("decode image"));
}
