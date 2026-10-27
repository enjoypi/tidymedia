#[path = "config_expand.rs"]
mod config_expand;
#[path = "config_io.rs"]
mod config_io;
#[path = "config_sanitize.rs"]
mod config_sanitize;

use tracing::debug;
use tracing::warn;

use crate::usecases::config::Config;

pub(super) use config_expand::expand_env;
#[cfg(test)]
pub(super) use config_expand::resolve_var;
pub use config_io::install_global_loader;
pub(super) use config_sanitize::sanitize;

pub(crate) fn load() -> Result<Config, String> {
    let path = config_io::config_path();
    let raw = match config_io::read(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !config_io::is_explicit() => {
            debug!(
                feature = "config",
                operation = "load",
                result = "fallback_default",
                path = %path,
                "config file missing, using defaults"
            );
            return Ok(Config::default());
        }
        Err(e) => {
            warn!(
                feature = "config",
                operation = "load",
                result = "read_error",
                path = %path,
                error = %e,
                "config read failed"
            );
            return Err(format!("cannot read config file {path}: {e}"));
        }
    };
    let expanded = expand_env(&raw);
    match config_io::parse(&expanded) {
        Ok(cfg) => {
            debug!(
                feature = "config",
                operation = "load",
                result = "ok",
                path = %path,
                "config loaded"
            );
            Ok(sanitize(cfg))
        }
        Err(e) => {
            warn!(
                feature = "config",
                operation = "load",
                result = "parse_error",
                path = %path,
                error = %e,
                "config parse failed"
            );
            Err(format!("cannot parse config file {path}: {e}"))
        }
    }
}

#[cfg(test)]
#[path = "config_fixture_tests.rs"]
mod test_common;

#[cfg(test)]
#[path = "config_expand_tests.rs"]
mod expand_tests;

#[cfg(test)]
#[path = "config_load_tests.rs"]
mod load_tests;
