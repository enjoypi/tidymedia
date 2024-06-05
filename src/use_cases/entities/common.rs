use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("fs_extra error occurred: {0}")]
    FsExtra(#[from] fs_extra::error::Error),

    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
