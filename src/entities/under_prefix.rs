#[must_use]
pub(crate) fn under_prefix(path: &str, prefix: &str) -> bool {
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
