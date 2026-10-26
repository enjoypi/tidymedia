use std::io;
use std::path::Path;

pub(super) fn read_to_string(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(path)
}
