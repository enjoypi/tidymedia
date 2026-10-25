//! `under_prefix` 外置：`rest.starts_with('\\')` 的 False edge 在 per-instance 中
//! 计 0（Unix 路径无反斜杠时该分支结构性不可达，Windows 反斜杠测试只在单 instance
//! 命中 True），供 ignore-regex 整文件排除。

#[must_use]
pub fn under_prefix(path: &str, prefix: &str) -> bool {
    let prefix = prefix.strip_suffix(['/', '\\']).unwrap_or(prefix);
    let Some((head, rest)) = path.split_at_checked(prefix.len()) else {
        return false;
    };
    if !same_path_text(head, prefix) {
        return false;
    }
    if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') {
        return true;
    }
    false
}

#[cfg(windows)]
fn same_path_text(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(not(windows))]
fn same_path_text(a: &str, b: &str) -> bool {
    a == b
}
