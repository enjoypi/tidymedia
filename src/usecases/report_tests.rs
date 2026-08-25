use super::*;

fn err(path: &str) -> ReportError {
    ReportError {
        path: path.to_owned(),
        message: "m".to_owned(),
    }
}

/// cap 未超时正常 push；truncated 不翻转。
#[test]
fn push_error_capped_under_cap() {
    let mut v = Vec::new();
    let mut t = false;
    push_error_capped(&mut v, &mut t, err("a"));
    assert_eq!(v.len(), 1);
    assert!(!t);
}

/// cap 达到时丢弃新 err 并置 truncated=true；Vec 长度保持 cap。
#[test]
fn push_error_capped_at_cap_sets_truncated() {
    let mut v = (0..ERRORS_SOFT_CAP)
        .map(|i| err(&format!("f{i}")))
        .collect();
    let mut t = false;
    push_error_capped(&mut v, &mut t, err("over"));
    assert_eq!(v.len(), ERRORS_SOFT_CAP);
    assert!(t);
}

/// `extend_errors_capped` 的 `src_truncated=true` arm：即便 dst 未满，
/// src 声明自身已 truncate 也应传染让 `dst_truncated=true`（防 delta 侧
/// 早已丢失记录但 merge 后误报"完整"）。
#[test]
fn extend_errors_capped_propagates_src_truncated() {
    let mut dst = Vec::new();
    let mut dst_t = false;
    extend_errors_capped(
        &mut dst,
        &mut dst_t,
        vec![err("a")],
        /* src_truncated = */ true,
    );
    assert_eq!(dst.len(), 1);
    assert!(dst_t, "src_truncated=true 必须传染到 dst_truncated");
}

/// `extend_errors_capped` 的 `src_truncated=false` 常规路径：合并 src 全量到 dst 且
/// `dst_truncated` 保持 false。
#[test]
fn extend_errors_capped_keeps_false_when_neither_full() {
    let mut dst = vec![err("prev")];
    let mut dst_t = false;
    extend_errors_capped(&mut dst, &mut dst_t, vec![err("a"), err("b")], false);
    assert_eq!(dst.len(), 3);
    assert!(!dst_t);
}

/// `feature_of(true)` = MOVE，`feature_of(false)` = COPY（双 arm 覆盖）。
#[test]
fn feature_of_maps_remove_flag() {
    assert_eq!(feature_of(true), FEATURE_MOVE);
    assert_eq!(feature_of(false), FEATURE_COPY);
}
