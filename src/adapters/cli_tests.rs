use super::config_level;
use super::run_cli;
use crate::usecases::config::Config;
use crate::usecases::config::install_loader;

#[test]
fn config_level_parses_valid_level() {
    assert_eq!(config_level("debug"), tracing::Level::DEBUG);
}

#[test]
fn config_level_falls_back_to_info_on_invalid() {
    assert_eq!(config_level("chatty"), tracing::Level::INFO);
}

fn failing_loader() -> Result<Config, String> {
    Err("bad yaml".to_string())
}

#[test]
fn run_cli_prechecks_config_before_dispatch() {
    install_loader(failing_loader);
    let err = run_cli(["tidymedia", "find", "tests/data"]).unwrap_err();
    assert!(format!("{err}").contains("bad yaml"), "got: {err}");
}

#[test]
fn run_cli_help_succeeds_with_failing_config() {
    install_loader(failing_loader);
    run_cli(["tidymedia", "--help"]).expect("help must not load config");
}
