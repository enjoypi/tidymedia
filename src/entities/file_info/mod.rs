pub(super) mod info;
pub(super) mod paths;
pub(super) mod streams;

pub use self::info::Info;
#[cfg(test)]
pub use self::paths::full_path;
pub(crate) use self::paths::strip_windows_unc;
pub(crate) use self::streams::read_fill;

#[cfg(test)]
use self::info::time::pick_fs_fallback;
#[cfg(test)]
use self::streams::{FAST_READ_SIZE, fast_hash_stream, full_hash_stream, secure_hash_stream};
#[cfg(test)]
use self::tests_common::{fast_hash, full_hash, secure_hash};
#[cfg(test)]
use crate::entities::SecureHash;

#[cfg(test)]
#[path = "file_info_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "file_info_error_tests.rs"]
mod error_tests;

#[cfg(test)]
#[path = "file_info_stream_tests.rs"]
mod stream_tests;

#[cfg(test)]
#[path = "file_info_backend_tests.rs"]
mod backend_tests;

#[cfg(test)]
#[path = "file_info_tests_common.rs"]
pub(crate) mod tests_common;
