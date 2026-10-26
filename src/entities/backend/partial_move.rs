use std::error::Error;
use std::fmt;
use std::io;

#[derive(Debug)]
pub struct PartialMove(String);

impl fmt::Display for PartialMove {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PartialMove {}

#[must_use]
pub(crate) fn partial_move_error(kind: io::ErrorKind, msg: String) -> io::Error {
    io::Error::new(kind, PartialMove(msg))
}

#[must_use]
pub(crate) fn is_partial_move(e: &io::Error) -> bool {
    matches!(e.get_ref(), Some(inner) if inner.is::<PartialMove>())
}

#[cfg(test)]
#[path = "partial_move_tests.rs"]
mod tests;
