//! 路径规范化工具：`full_path` 绝对化 + Windows UNC 前缀剥离。

use std::io;

use camino::Utf8Path;
use camino::Utf8PathBuf;

// `coverage(off)`：`if full.is_absolute()` 在集成 test binary 永远 True（lib_tidy
// 等用绝对路径），LLVM multi-binary 副本 False 分支不触发。语义由 lib unit
// 测试 full_path_absolute_passthrough / full_path_relative_canonicalizes 断言。
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn full_path(path: &str) -> io::Result<Utf8PathBuf> {
    let full = Utf8Path::new(path);
    if full.is_absolute() {
        return Ok(full.to_path_buf());
    }

    let full = full.canonicalize_utf8()?;
    Ok(Utf8PathBuf::from(strip_windows_unc(full.as_str())))
}

#[cfg(target_os = "windows")]
pub(super) fn strip_windows_unc(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn strip_windows_unc(path: &str) -> &str {
    path
}
