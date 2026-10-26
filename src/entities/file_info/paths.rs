#[path = "paths_io.rs"]
mod paths_io;

use std::io;

use camino::Utf8Path;
use camino::Utf8PathBuf;

pub fn full_path(path: &str) -> io::Result<Utf8PathBuf> {
    let full = Utf8Path::new(path);
    if full.is_absolute() {
        return Ok(full.to_path_buf());
    }
    let full = paths_io::canonicalize(full)?;
    Ok(Utf8PathBuf::from(strip_windows_unc(full.as_str())))
}

#[cfg(target_os = "windows")]
pub(crate) fn strip_windows_unc(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn strip_windows_unc(path: &str) -> &str {
    path
}
