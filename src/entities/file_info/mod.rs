//! `file_info` 模块：`Info` 实体 / 流式哈希 / 路径工具，按单一职责拆分子模块。
//! 对外路径（`file_info::{Info, full_path, read_fill, *_hash_stream}`）经 re-export 保持不变。

pub(super) mod info;
pub(super) mod paths;
pub(super) mod streams;

pub use self::info::Info;
pub use self::paths::full_path;
pub(crate) use self::streams::read_fill;

// 测试经 `super::X` 访问的内部项（私有 use 对子模块可见，生产侧不暴露）。
#[cfg(test)]
use self::info::pick_fs_fallback;
#[cfg(test)]
use self::paths::strip_windows_unc;
#[cfg(test)]
use self::streams::{
    FAST_READ_SIZE, fast_hash, fast_hash_stream, full_hash, full_hash_stream, secure_hash,
    secure_hash_stream,
};
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
