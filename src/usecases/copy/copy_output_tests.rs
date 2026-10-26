use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::tests_common::*;
use super::*;
use crate::adapters::report_sink::JsonFileReportSink;
use crate::entities::backend::Backend;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;
use crate::usecases::report::ReportSink;

#[test]
fn do_copy_propagates_stream_copy_error_when_open_write_fails() {
    use crate::adapters::backend::fake::{FakeBackend, Op};
    use std::io::ErrorKind;

    let src_dir = tempdir().unwrap();
    let info = make_media_info(src_dir.path(), "photo.png");

    let fake_out = std::sync::Arc::new(FakeBackend::new("local"));
    let out_loc = Location::Local(Utf8PathBuf::from("/fake_out"));
    fake_out.add_file(out_loc.clone(), vec![]);
    let target_loc = Location::Local(Utf8PathBuf::from("/fake_out/2024/01/photo.png"));
    fake_out.inject_error(target_loc, Op::OpenWrite, ErrorKind::PermissionDenied);

    let idx = crate::entities::file_index::Index::new();
    let backend_arc: std::sync::Arc<dyn Backend> = fake_out;
    let err = do_copy(
        &info,
        &out_loc,
        &backend_arc,
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .unwrap_err();
    let _ = err;
}

#[test]
fn do_copy_succeeds_when_dst_open_read_would_fail() {
    use crate::adapters::backend::fake::{FakeBackend, Op};
    use std::io::ErrorKind;

    let src_dir = tempdir().unwrap();
    let info = make_media_info(src_dir.path(), "photo.png");

    let fake_out = std::sync::Arc::new(FakeBackend::new("local"));
    let out_loc = Location::Local(Utf8PathBuf::from("/fake_out"));
    fake_out.add_file(out_loc.clone(), vec![]);

    let target_loc = Location::Local(Utf8PathBuf::from("/fake_out/2024/01/photo.png"));
    fake_out.inject_error(target_loc, Op::OpenRead, ErrorKind::Interrupted);

    let idx = crate::entities::file_index::Index::new();
    let backend_arc: std::sync::Arc<dyn Backend> = fake_out;
    let ok = do_copy(
        &info,
        &out_loc,
        &backend_arc,
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .expect("dst OpenRead Err 不再阻断 copy（cloned_at 路径零 IO）");
    assert!(ok, "do_copy 应报告 copy 成功");
}

#[test]
fn do_copy_cross_backend_move_removes_src_after_stream_copy() {
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let src_dir = tempdir().unwrap();
    let info = make_media_info(src_dir.path(), "photo.png");
    let src_path = src_dir.path().join("photo.png");

    let fake_out = Arc::new(FakeBackend::new("smb"));
    let out_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("out"),
    };
    fake_out.add_dir(out_loc.clone());

    let idx = crate::entities::file_index::Index::new();
    let backend_arc: Arc<dyn Backend> = fake_out;
    let opts = CopyOpts {
        dry_run: false,
        remove: true,
        include_non_media: false,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let ok = do_copy(&info, &out_loc, &backend_arc, &idx, &opts).unwrap();
    assert!(ok, "stream_copy should succeed");
    assert!(
        !src_path.exists(),
        "src must be removed after cross-backend move"
    );
}

#[test]
fn mkdir_cache_skips_repeated_mkdir_for_same_target_dir() {
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let src_dir = tempdir().unwrap();
    tc::copy_png_to(src_dir.path(), "a.png").unwrap();
    tc::copy_png_to(src_dir.path(), "b.png").unwrap();

    let fake_out = Arc::new(FakeBackend::new("smb"));
    let out_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("out"),
    };
    fake_out.add_dir(out_loc.clone());
    let fake_out_handle = Arc::clone(&fake_out);
    let backend_arc: Arc<dyn Backend> = fake_out;

    copy(
        &[local_source(src_dir.path())],
        (out_loc, backend_arc),
        false,
        false,
        false,
        None,
        None,
    )
    .unwrap();

    let calls = fake_out_handle.mkdir_p_calls();
    assert_eq!(
        calls, 2,
        "expected 2 mkdir_p calls (1 output root + 1 target dir, cache reuses target); got {calls}"
    );
}

#[test]
fn copy_empty_source_with_report_writes_zero_counts() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("empty_report.json");
    let sink = JsonFileReportSink::new(report_path.to_str().unwrap());
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        false,
        None,
        Some(&sink as &dyn ReportSink),
    )
    .unwrap();
    let content = fs::read_to_string(&report_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["scanned"], 0);
    assert_eq!(parsed["copied"], 0);
}
