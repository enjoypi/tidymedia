#[cfg(test)]
use std::io;

#[cfg(test)]
use camino::Utf8Path;
#[cfg(test)]
use camino::Utf8PathBuf;

#[cfg(test)]
pub(super) fn canonicalize(path: &Utf8Path) -> io::Result<Utf8PathBuf> {
    path.canonicalize_utf8()
}
