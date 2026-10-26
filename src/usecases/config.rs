use std::sync::Mutex;
use std::sync::PoisonError;

use chrono::{FixedOffset, Offset, Utc};
use serde_derive::Deserialize;
use time::UtcOffset;

#[path = "config_models.rs"]
mod config_models;
#[allow(unused_imports)]
pub use config_models::{
    BackendConfig, CategoryDef, ClassifyConfig, CopyConfig, ExifConfig, LogConfig, OcrConfig,
    SmbBackendConfig, VerifyConfig,
};

static CONFIG: Mutex<Option<&'static Config>> = Mutex::new(None);
static LOADER: Mutex<Option<fn() -> Config>> = Mutex::new(None);

pub fn config() -> &'static Config {
    let mut guard = CONFIG.lock().unwrap_or_else(PoisonError::into_inner);
    guard.get_or_insert_with(|| {
        let ldr = LOADER.lock().unwrap_or_else(PoisonError::into_inner);
        let loader: Option<fn() -> Config> = *ldr;
        Box::leak(Box::new(loader.map_or_else(Config::default, |f| f())))
    })
}

pub fn install_loader(loader: fn() -> Config) {
    *LOADER.lock().unwrap_or_else(PoisonError::into_inner) = Some(loader);
}

#[must_use]
pub fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

#[must_use]
pub fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| Utc.fix())
}

pub const DEFAULT_ARCHIVE_TEMPLATE: &str = "{year}/{month}/{valuable_name}";
pub const DEFAULT_DOC_ARCHIVE_TEMPLATE: &str = "{category}/{year}/{month}";

pub fn validate_archive_template(template: &str) -> Result<(), String> {
    const ALWAYS_NON_EMPTY: [&str; 6] = ["year", "month", "day", "make", "model", "category"];
    if template.is_empty() {
        return Err("archive_template must not be empty".into());
    }
    let mut start: Option<usize> = None;
    let mut has_safe_placeholder = false;
    for (i, c) in template.char_indices() {
        match c {
            '{' if start.is_some() => {
                return Err("archive_template has unbalanced braces: nested '{'".into());
            }
            '{' => start = Some(i + 1),
            '}' => {
                let Some(s) = start.take() else {
                    return Err("archive_template has unbalanced braces: unmatched '}'".into());
                };
                let name = &template[s..i];
                if !crate::usecases::archive_template::PLACEHOLDERS.contains(&name) {
                    return Err(format!(
                        "archive_template has unknown placeholder {{{name}}}"
                    ));
                }
                if ALWAYS_NON_EMPTY.contains(&name) {
                    has_safe_placeholder = true;
                }
            }
            _ => {}
        }
    }
    if start.is_some() {
        return Err("archive_template has unbalanced braces: unclosed '{'".into());
    }
    if !has_safe_placeholder {
        return Err("archive_template must contain at least one of \
             {year}/{month}/{day}/{make}/{model} to guarantee a non-empty subdirectory"
            .into());
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub copy: CopyConfig,
    pub exif: ExifConfig,
    pub verify: VerifyConfig,
    pub backend: BackendConfig,
    pub log: LogConfig,
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
