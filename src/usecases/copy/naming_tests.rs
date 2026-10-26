use super::*;
use std::time::Duration;

#[test]
fn system_time_to_offsetdatetime_pre_epoch_returns_none() {
    let pre = UNIX_EPOCH.checked_sub(Duration::from_secs(1));
    // 部分 Windows/CI 环境 SystemTime 不支持 pre-epoch 构造 → checked_sub 返 None
    // 直接跳过（本测试仅关注 pre-epoch 可构造时的路径命中）。
    let Some(t) = pre else {
        return;
    };
    assert!(system_time_to_offsetdatetime(t).is_none());
}

/// `UNIX_EPOCH` 本身 → `Ok(OffsetDateTime::UNIX_EPOCH)`（首正常路径回归）。
#[test]
fn system_time_to_offsetdatetime_at_epoch_returns_epoch() {
    let got = system_time_to_offsetdatetime(UNIX_EPOCH).expect("UNIX_EPOCH is valid");
    assert_eq!(got.unix_timestamp(), 0);
}

/// 常规多段相对路径一次挂到 output 下（多段 `join_path` 文档化行为）。
#[test]
fn build_sub_dir_joins_multi_segment_rel_path() {
    let out = Location::Local(camino::Utf8PathBuf::from("/out"));
    let got = build_sub_dir(&out, "2024/01");
    assert!(std::path::Path::new(got.display().as_str()).ends_with("2024/01"));
}

/// `..` / `.` / 空段被第二道防线剥除，不逃逸 output 根。
#[test]
fn build_sub_dir_strips_dot_and_dotdot_segments() {
    let out = Location::Local(camino::Utf8PathBuf::from("/out"));
    let got = build_sub_dir(&out, "../2024/./01//");
    assert!(std::path::Path::new(got.display().as_str()).ends_with("2024/01"));
    assert!(!got.display().contains(".."));
}

/// 相对路径全部被剥除时回落 output 本身（不产生尾随分隔符脏路径）。
#[test]
fn build_sub_dir_all_segments_stripped_falls_back_to_output() {
    let out = Location::Local(camino::Utf8PathBuf::from("/out"));
    let got = build_sub_dir(&out, "../..");
    assert_eq!(got.display(), out.display());
}

#[test]
fn convert_dur_secs_rejects_u64_beyond_i64() {
    assert!(convert_dur_secs_to_offsetdatetime(u64::MAX).is_none());
}
