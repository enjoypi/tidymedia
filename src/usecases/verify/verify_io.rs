use std::io::{self, Read};
use std::path::Path;

pub(super) fn read_to_string(path: &Path, max_bytes: u64) -> io::Result<String> {
    if path == Path::new("-") {
        read_stream(std::io::stdin().lock(), max_bytes)
    } else {
        read_stream(std::fs::File::open(path)?, max_bytes)
    }
}

pub(super) fn read_stream<R: Read>(reader: R, max_bytes: u64) -> io::Result<String> {
    let mut content = String::new();
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_string(&mut content)?;
    if content.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("exif tsv exceeds {max_bytes} bytes"),
        ));
    }
    Ok(content)
}

#[cfg(test)]
#[path = "verify_io_tests.rs"]
mod tests;
