use super::test_common::make_scanned;
use super::*;

/// 单元素组 → skip（line 127 True arm）。
#[test]
fn filter_multi_image_groups_skips_singleton() {
    let scanned = vec![make_scanned(200.0)];
    let groups = vec![vec![0]];
    let (kept, dropped) = filter_multi_image_groups(groups, &scanned, 0.0);
    assert_eq!(kept, [] as [Vec<usize>; 0]);
    assert_eq!(dropped, 0);
}

/// 多图组经 `filter_blurry` 剩 <2 → skip（line 135 True arm）；同时统计 dropped 到总数。
#[test]
fn filter_multi_image_groups_skips_when_filtered_below_two() {
    let scanned = vec![make_scanned(50.0), make_scanned(200.0)];
    let groups = vec![vec![0, 1]];
    let (kept, dropped) = filter_multi_image_groups(groups, &scanned, 100.0);
    assert!(kept.is_empty(), "filter 后仅剩 1 张 → 整组丢弃");
    assert_eq!(dropped, 1);
}

/// 合规多图组 → 保留（正常路径）。
#[test]
fn filter_multi_image_groups_keeps_two_plus_after_filter() {
    let scanned = vec![make_scanned(200.0), make_scanned(300.0), make_scanned(50.0)];
    let groups = vec![vec![0, 1, 2]];
    let (kept, dropped) = filter_multi_image_groups(groups, &scanned, 100.0);
    assert_eq!(kept, vec![vec![0, 1]]);
    assert_eq!(dropped, 1);
}

#[test]
fn filter_blurry_early_returns_when_min_not_finite() {
    let scanned = [make_scanned(50.0), make_scanned(200.0)];
    let (kept, dropped) = filter_blurry(&[0, 1], &scanned, f32::NAN);
    assert_eq!(kept, vec![0, 1]);
    assert_eq!(dropped, 0);
}

#[test]
fn filter_blurry_early_returns_when_min_non_positive() {
    let scanned = [make_scanned(50.0), make_scanned(200.0)];
    let (kept, dropped) = filter_blurry(&[0, 1], &scanned, 0.0);
    assert_eq!(kept, vec![0, 1]);
    assert_eq!(dropped, 0);
}

#[test]
fn filter_blurry_drops_below_min_keeps_nan_and_ge_min() {
    let scanned = [
        make_scanned(200.0),
        make_scanned(50.0),
        make_scanned(f32::NAN),
    ];
    let (kept, dropped) = filter_blurry(&[0, 1, 2], &scanned, 100.0);
    assert_eq!(kept, vec![0, 2]);
    assert_eq!(dropped, 1);
}

#[test]
fn total_cmp_nan_as_neg_inf_covers_all_arms() {
    use std::cmp::Ordering;
    assert_eq!(
        total_cmp_nan_as_neg_inf(f32::NAN, f32::NAN),
        Ordering::Equal
    );
    assert_eq!(total_cmp_nan_as_neg_inf(f32::NAN, 1.0), Ordering::Less);
    assert_eq!(total_cmp_nan_as_neg_inf(1.0, f32::NAN), Ordering::Greater);
    assert_eq!(total_cmp_nan_as_neg_inf(1.0, 2.0), Ordering::Less);
}
