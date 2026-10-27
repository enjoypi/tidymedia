use tempfile::tempdir;
use tidymedia::{BackendFactory, Commands, DefaultBackendFactory, DefaultDetectorFactory};
use tidymedia::{DetectorFactory, run_cli, tidy, tidy_with};

use super::FakeBackendFactory;
use super::{DATA_DIR, local};

fn write_bad_yaml_config() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("bad.yaml");
    std::fs::write(&cfg_path, "::: not yaml :::").unwrap();
    unsafe { std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap()) };
    tidymedia::install_config_loader();
    dir
}

#[test]
fn run_cli_fails_when_explicit_config_file_missing() {
    unsafe { std::env::set_var("TIDYMEDIA_CONFIG", "/no/such/tidymedia-config.yaml") };
    tidymedia::install_config_loader();
    let err = run_cli(["tidymedia", "find", DATA_DIR]).unwrap_err();
    assert!(
        format!("{err}").contains("/no/such/tidymedia-config.yaml"),
        "missing explicit config must fail-fast, got: {err}"
    );
}

#[test]
fn tidy_fails_when_config_yaml_invalid() {
    let _dir = write_bad_yaml_config();
    let err = tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: None,
        report: None,
    })
    .unwrap_err();
    assert!(
        format!("{err}").contains("cannot parse config file"),
        "invalid yaml must fail-fast, got: {err}"
    );
}

#[test]
fn tidy_with_runs_config_precheck_before_factory_dispatch() {
    let _dir = write_bad_yaml_config();
    let factory = FakeBackendFactory::new();
    let smb_out = tidymedia::Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: camino::Utf8PathBuf::from("out"),
    };
    let err = tidy_with(
        &factory,
        &DefaultDetectorFactory,
        Commands::Find {
            secure: false,
            sources: vec![local(DATA_DIR)],
            output: Some(smb_out),
            report: None,
        },
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("cannot parse config file"),
        "precheck must fail first, got: {msg}"
    );
    assert!(
        !msg.contains("no fake backend for scheme"),
        "factory error means precheck ran too late: {msg}"
    );
}

#[test]
fn default_detector_factory_propagates_config_error() {
    let _dir = write_bad_yaml_config();
    let err = DefaultDetectorFactory.build_text_detector().unwrap_err();
    assert!(
        err.to_string().contains("cannot parse config file"),
        "detector factory must propagate load error, got: {err}"
    );
    let err = DefaultDetectorFactory
        .build_document_classifier()
        .unwrap_err();
    assert!(
        err.to_string().contains("cannot parse config file"),
        "classifier factory must propagate load error, got: {err}"
    );
}

#[test]
fn default_backend_factory_propagates_config_error() {
    let _dir = write_bad_yaml_config();
    let smb = tidymedia::Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: camino::Utf8PathBuf::from("out"),
    };
    let err = DefaultBackendFactory
        .for_location(&smb)
        .err()
        .expect("backend factory must fail on config error");
    assert!(
        format!("{err}").contains("cannot parse config file"),
        "backend factory must propagate load error, got: {err}"
    );
}

#[test]
fn run_cli_help_succeeds_with_invalid_config() {
    let _dir = write_bad_yaml_config();
    run_cli(["tidymedia", "--help"]).expect("help must not load config");
}
