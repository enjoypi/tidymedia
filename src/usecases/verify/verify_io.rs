use std::io::{self, Read};
use std::path::Path;

pub(super) fn read_to_string(path: &Path) -> io::Result<String> {
    if path == Path::new("-") {
        read_stream(std::io::stdin().lock())
    } else {
        read_stream(std::fs::File::open(path)?)
    }
}

pub(super) fn read_stream<R: Read>(mut reader: R) -> io::Result<String> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    Ok(content)
}

#[cfg(test)]
#[path = "verify_io_tests.rs"]
mod tests;
