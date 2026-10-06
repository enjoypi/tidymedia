use std::error::Error;
use std::fmt;
use std::io;

#[derive(Debug)]
struct OutputWrite(io::Error);

impl fmt::Display for OutputWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for OutputWrite {}

#[must_use]
pub fn output_write_error(e: io::Error) -> io::Error {
    io::Error::new(e.kind(), OutputWrite(e))
}

#[must_use]
pub fn is_output_write_denied(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied
        && matches!(e.get_ref(), Some(inner) if inner.is::<OutputWrite>())
}

#[cfg(test)]
#[path = "output_write_tests.rs"]
mod tests;
