#[test]
fn copy_dispatch_accepts_explicit_archive_template() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report = out.path().join("report.json");
    let result = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Copy {
            dry_run: true,
            include_non_media: false,
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            archive_template: Some("{year}/{month}".to_string()),
            report: Some(report.to_string_lossy().into_owned()),
        },
    );
    assert!(result.is_ok());
    assert!(report.exists());
}

#[test]
fn copy_dispatch_rejects_invalid_archive_template() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let result = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Copy {
            dry_run: true,
            include_non_media: false,
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            archive_template: Some("{year/{month}".to_string()),
            report: None,
        },
    );
    assert!(result.is_err());
}

#[test]
fn copy_doc_dispatch_covers_category_and_plain_templates() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    for template in ["{year}/{month}", "{category}/{year}"] {
        let result = tidy_with(
            &LocalOnlyFactory,
            &StubDetectorFactory,
            Commands::CopyDoc {
                dry_run: true,
                sources: vec![local_loc_str(src.path())],
                output: local_loc_str(out.path()),
                archive_template: Some(template.to_string()),
                report: None,
            },
        );
        assert!(result.is_ok());
    }
}

#[test]
fn dispatch_commands_accept_missing_report_path() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let find = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![local_loc_str(src.path())],
            output: Some(local_loc_str(out.path())),
            report: None,
        },
    );
    assert!(find.is_ok());
    let move_text = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::MoveTextShot {
            dry_run: true,
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            report: None,
        },
    );
    assert!(move_text.is_ok());
    let verify = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Verify {
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            include_non_media: false,
            exif_tsv: None,
            phash_max: None,
            report: None,
        },
    );
    assert!(verify.is_ok());
}

#[test]
fn dispatch_report_failures_propagate() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report = "/nonexistent_dir_xyz/report.json".to_string();
    let find = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![local_loc_str(src.path())],
            output: Some(local_loc_str(out.path())),
            report: Some(report.clone()),
        },
    );
    assert!(find.is_err());
    let move_text = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::MoveTextShot {
            dry_run: true,
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            report: Some(report.clone()),
        },
    );
    assert!(move_text.is_err());
    let verify = tidy_with(
        &LocalOnlyFactory,
        &StubDetectorFactory,
        Commands::Verify {
            sources: vec![local_loc_str(src.path())],
            output: local_loc_str(out.path()),
            include_non_media: false,
            exif_tsv: None,
            phash_max: None,
            report: Some(report),
        },
    );
    assert!(verify.is_err());
}
