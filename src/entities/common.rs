use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn io_error_display_contains_inner_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err: Error = io_err.into();
        let msg = format!("{}", err);
        assert!(msg.starts_with("IO error occurred:"), "got: {msg}");
        assert!(msg.contains("no such file"), "got: {msg}");
    }
}
