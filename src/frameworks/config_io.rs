use std::env;
use std::fs;
use std::io;

use crate::usecases::config::Config;

pub fn install_global_loader() {
    crate::usecases::config::install_loader(super::load);
}

pub(super) fn config_path() -> String {
    env::var("TIDYMEDIA_CONFIG").unwrap_or_else(|_| "config.yaml".to_string())
}

pub(super) fn env_var(name: &str) -> Result<String, env::VarError> {
    env::var(name)
}

pub(super) fn is_explicit() -> bool {
    env_var("TIDYMEDIA_CONFIG").is_ok()
}

pub(super) fn read(path: &str) -> io::Result<String> {
    fs::read_to_string(path)
}

pub(super) fn parse(raw: &str) -> Result<Config, serde_yaml::Error> {
    serde_yaml::from_str(raw)
}
