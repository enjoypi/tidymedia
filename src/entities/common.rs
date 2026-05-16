use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("fs_extra error occurred: {0}")]
    FsExtra(#[from] fs_extra::error::Error),

    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse an json: {0}")]
    ParseJson(#[from] serde_json::Error),
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

    #[test]
    fn parse_json_display_contains_inner_message() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: Error = json_err.into();
        let msg = format!("{}", err);
        assert!(msg.starts_with("Failed to parse an json:"), "got: {msg}");
    }

    #[test]
    fn fs_extra_display_contains_inner_message() {
        let inner = fs_extra::error::Error::new(
            fs_extra::error::ErrorKind::NotFound,
            "missing path",
        );
        let err: Error = inner.into();
        let msg = format!("{}", err);
        assert!(msg.starts_with("fs_extra error occurred:"), "got: {msg}");
        assert!(msg.contains("missing path"), "got: {msg}");
    }
}
