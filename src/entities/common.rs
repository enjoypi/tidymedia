use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// 判 path 是否在 prefix 目录下（或恰等于 prefix）：纯 `starts_with` 会让
/// `/photos_backup` 误判为 `/photos` 子目录，必须额外校验 prefix 后紧跟路径
/// 分隔符。Unix 用 `/`，Windows 兼顾 `\`。
#[must_use]
pub fn under_prefix(path: &str, prefix: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }
    let rest = &path[prefix.len()..];
    rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn io_error_display_contains_inner_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err: Error = io_err.into();
        let msg = format!("{err}");
        assert!(msg.starts_with("IO error occurred:"), "got: {msg}");
        assert!(msg.contains("no such file"), "got: {msg}");
    }
}
