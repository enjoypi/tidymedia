use std::sync::Arc;

use tempfile::tempdir;
use tidymedia::{CommandResult, Commands, DefaultDetectorFactory, FakeBackend, FakeOp, tidy_with};

use super::{FakeBackendFactory, local, smb_loc};

#[test]
fn smb_copy_reader_error_treats_file_as_skipped() {
    let smb_root = smb_loc("DCIM");
    let smb_file = smb_loc("DCIM/flaky.jpg");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(smb_file.clone(), vec![0x11; 8192]);
    fake_smb.inject_reader_error(smb_file, std::io::ErrorKind::TimedOut);

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    let result = tidy_with(
        &factory,
        &DefaultDetectorFactory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![smb_root],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy should swallow reader error and finalize Ok");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert!(
        report.skipped_unreadable >= 1,
        "reader error must be counted in skipped_unreadable: {report:?}"
    );
    assert!(
        std::fs::read_dir(out_dir.path()).unwrap().next().is_none(),
        "no file should land in out when source unreadable"
    );
}

#[test]
fn smb_copy_mixed_files_one_failure_does_not_block_others() {
    let smb_root = smb_loc("DCIM");
    let good_file = smb_loc("DCIM/good.jpg");
    let bad_file = smb_loc("DCIM/bad.jpg");

    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());
    fake_smb.add_file(good_file, b"GOOD-BYTES-1234567890".to_vec());
    fake_smb.add_file(bad_file.clone(), b"BAD-BYTES-1234567890".to_vec());
    fake_smb.inject_error(
        bad_file,
        FakeOp::OpenRead,
        std::io::ErrorKind::PermissionDenied,
    );

    let out_dir = tempdir().unwrap();
    let mut factory = FakeBackendFactory::new();
    factory.insert("smb", fake_smb);

    let result = tidy_with(
        &factory,
        &DefaultDetectorFactory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![smb_root],
            output: local(out_dir.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy should continue past per-file failure");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    let good_dst = out_dir.path().join("1970").join("01").join("good.jpg");
    assert!(
        good_dst.exists(),
        "good file must reach out: {good_dst:?}; report={report:?}"
    );
    assert!(
        report.skipped_unreadable >= 1,
        "bad file must be counted as skipped_unreadable: {report:?}"
    );
}
