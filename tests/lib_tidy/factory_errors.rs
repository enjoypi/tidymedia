use tempfile::tempdir;
use tidymedia::{
    BackendFactory, Commands, DefaultBackendFactory, DefaultDetectorFactory, tidy_with,
};

use super::{FakeBackendFactory, local, smb_loc};

#[test]
fn factory_smb_for_location_builds_real_backend_or_reports_smb2_error() {
    match DefaultBackendFactory.for_location(&smb_loc("Inbox")) {
        Ok(backend) => assert_eq!(backend.scheme(), "smb"),
        Err(e) => {
            let msg = format!("{e}");
            assert!(msg.contains("smb2"), "got: {msg}");
        }
    }
}

#[test]
fn tidy_find_smb_source_propagates_factory_error() {
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![smb_loc("DCIM")],
            output: None,
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}

#[test]
fn tidy_find_smb_output_propagates_factory_error() {
    let src = tempdir().unwrap();
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: Some(smb_loc("Out")),
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}

#[test]
fn tidy_copy_smb_source_propagates_factory_error() {
    let out = tempdir().unwrap();
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Copy {
            dry_run: true,
            include_non_media: false,
            sources: vec![smb_loc("DCIM")],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}

#[test]
fn tidy_copy_smb_output_propagates_factory_error() {
    let src = tempdir().unwrap();
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Copy {
            dry_run: true,
            include_non_media: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: smb_loc("Out"),
            archive_template: None,
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}

#[test]
fn tidy_move_smb_source_propagates_factory_error() {
    let out = tempdir().unwrap();
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Move {
            dry_run: true,
            include_non_media: false,
            sources: vec![smb_loc("DCIM")],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}

#[test]
fn tidy_move_smb_output_propagates_factory_error() {
    let src = tempdir().unwrap();
    let err = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        Commands::Move {
            dry_run: true,
            include_non_media: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: smb_loc("Out"),
            archive_template: None,
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no fake backend for scheme smb"), "got: {msg}");
}
