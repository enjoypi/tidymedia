use std::fs;
use std::io;
use std::path::Path;

use crate::entities::backend::is_partial_move;

pub(super) fn reserve(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(drop)
}

pub(super) fn rename_noreplace_with(
    from: &Path,
    to: &Path,
    inner: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    reserve(to)?;
    inner(from, to).inspect_err(|e| {
        if !is_partial_move(e) {
            let _ = fs::remove_file(to);
        }
    })
}

pub(super) fn copy_noreplace(from: &Path, to: &Path) -> io::Result<u64> {
    reserve(to)?;
    fs::copy(from, to).inspect_err(|_| {
        let _ = fs::remove_file(to);
    })
}
