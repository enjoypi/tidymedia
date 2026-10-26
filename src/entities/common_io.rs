use std::io;

use camino::Utf8Path;

pub(super) fn canonicalize_path(path: &Utf8Path) -> io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path.as_std_path())
}

pub(super) fn canonicalize(path: &Utf8Path) -> io::Result<String> {
    canonicalize_path(path).map(|p| p.to_string_lossy().into_owned())
}
