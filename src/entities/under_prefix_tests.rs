use super::under_prefix::under_prefix;

#[test]
fn under_prefix_matches_windows_backslash_rest() {
    assert!(under_prefix(r"C:\photos\img.jpg", r"C:\photos"));
    assert!(!under_prefix(r"C:\photos\img.jpg", r"C:\photoshare"));
}

#[test]
fn under_prefix_rejects_prefix_longer_than_path() {
    assert!(!under_prefix("C:", r"C:\photos"));
}

#[test]
#[cfg(windows)]
fn under_prefix_ignores_ascii_case_on_windows() {
    assert!(under_prefix(r"d:\user\Pictures\a.jpg", r"D:\User\pictures"));
}
