use std::fs;

use tempfile::tempdir;

use super::tests_common::*;
use super::*;
use crate::adapters::report_sink::JsonFileReportSink;
use crate::entities::test_common as tc;
use crate::usecases::report::ReportSink;

#[test]
fn copy_include_non_media_copies_plain_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("readme.txt"), b"hello world").unwrap();
    let out = tempdir().unwrap();
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        true,
        None,
        None,
    )
    .unwrap();
    let copied = walk_files(out.path());
    assert!(
        !copied.is_empty(),
        "include_non_media must copy non-media files"
    );
}

#[test]
fn do_copy_include_non_media_writes_non_media() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doc.txt"), b"plain document").unwrap();
    let out = tempdir().unwrap();
    let info = Info::from(src.path().join("doc.txt").to_str().unwrap()).unwrap();
    assert!(!info.is_media());
    let idx = crate::entities::file_index::Index::new();
    let opts = CopyOpts {
        dry_run: false,
        remove: false,
        include_non_media: true,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let did = do_copy(&info, &local_loc(out.path()), &local_arc(), &idx, &opts).unwrap();
    assert!(did, "non-media must be copied when include_non_media=true");
}

#[test]
fn do_copy_default_skips_non_media() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doc.txt"), b"plain document").unwrap();
    let out = tempdir().unwrap();
    let info = Info::from(src.path().join("doc.txt").to_str().unwrap()).unwrap();
    let idx = crate::entities::file_index::Index::new();
    let did = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .unwrap();
    assert!(!did);
    assert_eq!(walk_files(out.path()).len(), 0);
}

#[test]
fn generate_unique_name_empty_template_result_uses_output_dir() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    let (_, target) = generate_unique_name(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        "{valuable_name}",
        &crate::entities::file_index::Index::new(),
        false,
    )
    .unwrap()
    .expect("should generate even with empty subdir");
    assert!(
        target.display().ends_with("photo.png"),
        "got: {}",
        target.display()
    );
}

#[test]
fn copy_with_custom_archive_template_uses_day() {
    let src = tempdir().unwrap();
    let nested = src.path().join("test_album");
    fs::create_dir_all(&nested).unwrap();
    tc::copy_png_to(&nested, "photo.png").unwrap();
    let out = tempdir().unwrap();
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        false,
        Some("{year}/{month}/{day}"),
        None,
    )
    .unwrap();
    let expected = out
        .path()
        .join("2024")
        .join("01")
        .join("01")
        .join("photo.png");
    assert!(expected.exists(), "expected file at {expected:?}");
}

#[test]
fn copy_with_report_path_creates_valid_json() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("report.json");
    let sink = JsonFileReportSink::new(report_path.to_str().unwrap());
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        true,
        false,
        false,
        None,
        Some(&sink as &dyn ReportSink),
    )
    .unwrap();
    let content = fs::read_to_string(&report_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["dry_run"], true);
    assert!(parsed["scanned"].as_u64().unwrap() >= 1);
}

#[test]
#[cfg(unix)]
fn do_copy_propagates_mkdir_error_when_archive_year_is_blocked() {
    use std::os::unix::fs::PermissionsExt;
    let out_dir = tempdir().unwrap();
    let mut perm = fs::metadata(out_dir.path()).unwrap().permissions();
    perm.set_mode(0o555);
    fs::set_permissions(out_dir.path(), perm).unwrap();

    let src_dir = tempdir().unwrap();
    let info = make_media_info(src_dir.path(), "photo.png");
    let idx = crate::entities::file_index::Index::new();

    let err = do_copy(
        &info,
        &local_loc(out_dir.path()),
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .unwrap_err();
    let _ = err;

    let mut perm = fs::metadata(out_dir.path()).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(out_dir.path(), perm).unwrap();
}
