use std::io;
use std::path::Path;

use crate::entities::backend::MappedFile;

pub(super) fn map_local(
    path: &Path,
    is_fixed_volume: fn(&Path) -> bool,
    map: fn(&Path) -> io::Result<Option<MappedFile>>,
) -> io::Result<Option<MappedFile>> {
    if !is_fixed_volume(path) {
        return Ok(None);
    }
    map(path)
}

#[cfg(test)]
#[path = "local_whole_tests.rs"]
mod tests;
