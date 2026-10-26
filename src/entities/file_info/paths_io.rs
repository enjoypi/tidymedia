use std::io;

use camino::Utf8Path;
use camino::Utf8PathBuf;

pub(super) fn canonicalize(path: &Utf8Path) -> io::Result<Utf8PathBuf> {
    path.canonicalize_utf8()
}
