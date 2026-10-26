#[path = "common_io.rs"]
mod common_io;
#[path = "under_prefix.rs"]
mod under_prefix;

use thiserror::Error;

use camino::Utf8Path;

use crate::entities::uri::Location;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) use self::under_prefix::under_prefix;

#[must_use]
pub fn canonical_prefix(loc: &Location) -> String {
    match loc {
        Location::Local(p) => match common_io::canonicalize(p) {
            Ok(s) => crate::entities::file_info::strip_windows_unc(&s).to_string(),
            Err(_) => p.as_str().to_string(),
        },
        other @ Location::Smb { .. } => other.display(),
    }
}

#[must_use]
pub(crate) fn canonical_paths_equal(a: &Utf8Path, b: &Utf8Path) -> bool {
    match (
        common_io::canonicalize_path(a),
        common_io::canonicalize_path(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[must_use]
pub fn entry_under_prefix(entry_loc: &Location, output_prefix: &str) -> bool {
    if under_prefix(&entry_loc.display(), output_prefix) {
        return true;
    }
    under_prefix(&canonical_prefix(entry_loc), output_prefix)
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "under_prefix_tests.rs"]
mod under_prefix_tests;
