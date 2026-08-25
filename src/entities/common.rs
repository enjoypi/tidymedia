use thiserror::Error;

use crate::entities::uri::Location;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[path = "common_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub use self::phantom::under_prefix;

/// 把 [`Location`] 规范化为 prefix 字符串：Local 路径直接 `std::fs::canonicalize`
/// **解析符号链接**（Windows UNC `\\?\` 前缀剥离与 `full_path` 同口径）；远端 backend
/// 直接 display。copy / move / cull / move-text-shot 4 个 use case 的「source 是否
/// 在 output 子树」判定共用此助手——朴素 `Location::display()` 在 output 是符号链接时
/// （`/tmp/out → /photos/out`）会让 src `/photos/out/img.jpg` 与 output prefix
/// `/tmp/out` 字面不匹配，`under_prefix` 误返 false，move 模式下源被当"output 外"
/// 被搬迁致循环或丢失。
///
/// 不复用 `file_info::full_path`：后者对 `is_absolute()` 路径直接返原样以避开
/// Windows 用户目录跨盘 canonicalize 陷阱（`C:\Users\x → D:\Users\x`），但那是
/// 「路径索引 key」语义（希望字面稳定），与 `canonical_prefix` 的「重叠判定 by 真实
/// 物理位置」语义相反——两 helper 独立维护语义正交，避免共用 `full_path` 让 symlink
/// 检查静默失效。canonicalize 失败（路径不存在，如 dry-run 到不存在的 output）时
/// fallback 到原字符串（保 CLI 早期语义）。
#[must_use]
pub fn canonical_prefix(loc: &Location) -> String {
    match loc {
        Location::Local(p) => match std::fs::canonicalize(p.as_std_path()) {
            Ok(std_path) => {
                let s = std_path.to_string_lossy();
                crate::entities::file_info::strip_windows_unc(&s).to_string()
            }
            Err(_) => p.as_str().to_string(),
        },
        other => other.display(),
    }
}

/// entry 级「是否位于 output 子树」判定：`output_prefix` 已 canonical，而 walker
/// yield 的 entry 路径是字面形式（含 symlink 段，如 macOS `/var → /private/var`、
/// `/tmp → /private/tmp`）→ 纯字面 `under_prefix` 会误返 false，让 output 子树
/// 文件被当 source 处理（cull/copy 就地归档保护失效）。字面 fast-path +
/// entry canonicalize 补判；远端 backend `canonical_prefix` fallback 到 display
/// 即等价。cull / move-text-shot 共用；copy 的 `Index::remove_under_prefix`
/// 场景（key 是字面 `&str` 非 `Location`）走双前缀方案（canonical + 字面各剔一遍）。
///
/// 字面 fast-path + canonical fallback 收敛在同一 helper，让上层调用点只剩单
/// branch，避免 fake 测试环境下「字面 false 但 canonical true」sub-branch 不可测
/// 的 phantom miss。
#[must_use]
pub fn entry_under_prefix(entry_loc: &Location, output_prefix: &str) -> bool {
    if under_prefix(&entry_loc.display(), output_prefix) {
        return true;
    }
    under_prefix(&canonical_prefix(entry_loc), output_prefix)
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
