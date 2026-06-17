use super::config;
use super::load;
use super::test_common::remove_env_var;
use super::test_common::set_env_var;

// yaml 故意保留已删除的 timeout_secs / mtp 节：serde 默认忽略未知字段，
// 旧 config.yaml 必须保持向后兼容不报错。
#[test]
fn backend_config_yaml_overrides_defaults_and_ignores_removed_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backend.yaml");
    std::fs::write(
        &path,
        "backend:\n  smb:\n    default_user: alice\n    workgroup: HOME\n    timeout_secs: 60\n  mtp:\n    device_match: exact\n    storage_match: exact\n  adb:\n    server_host: 10.0.0.5\n    server_port: 15037\n    timeout_secs: 90\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.backend.smb.default_user, "alice");
    assert_eq!(cfg.backend.smb.workgroup, "HOME");
    assert_eq!(cfg.backend.adb.server_host, "10.0.0.5");
    assert_eq!(cfg.backend.adb.server_port, 15037);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_falls_back_when_file_missing() {
    set_env_var("TIDYMEDIA_CONFIG", "/no/such/file/xyz.yaml");
    let cfg = load();
    assert_eq!(cfg.copy.timezone_offset_hours, 8);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_falls_back_when_yaml_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.yaml");
    std::fs::write(&path, "::: not yaml :::").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.unique_name_max_attempts, 10);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_reads_explicit_values_via_env_var() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ok.yaml");
    std::fs::write(
        &path,
        "copy:\n  timezone_offset_hours: 0\n  unique_name_max_attempts: 5\nexif:\n  valid_date_time_secs: 100\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.timezone_offset_hours, 0);
    assert_eq!(cfg.copy.unique_name_max_attempts, 5);
    assert_eq!(cfg.exif.valid_date_time_secs, 100);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn config_global_accessor_returns_static() {
    let a = config();
    let b = config();
    assert!(std::ptr::eq(a, b));
}

// max_attempts=0 会让 generate_unique_name 恒返 None（copy 静默全量失败），
// load 必须回退默认值。
#[test]
fn load_sanitizes_zero_unique_name_max_attempts_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.yaml");
    std::fs::write(&path, "copy:\n  unique_name_max_attempts: 0\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.unique_name_max_attempts, 10);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 超 ±23h 时区（chrono::FixedOffset / time::UtcOffset 在更大值上越界静默回退 UTC）：
// sanitize 必须 warn + 回退默认 8，避免月末文件按 UTC 解释跨月归错桶且无告警。
#[test]
fn load_sanitizes_out_of_range_timezone_offset_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badtz.yaml");
    std::fs::write(&path, "copy:\n  timezone_offset_hours: 26\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.timezone_offset_hours, 8);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 负方向同样越界（避免 sanitize 只检正方向漏掉 -26 等场景）。
#[test]
fn load_sanitizes_negative_out_of_range_timezone_offset_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badtz_neg.yaml");
    std::fs::write(&path, "copy:\n  timezone_offset_hours: -30\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.timezone_offset_hours, 8);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// yaml 内非法模板（结构错配）回退默认模板，不让渲染产生字面 '{' 目录。
#[test]
fn load_sanitizes_invalid_archive_template_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badtmpl.yaml");
    std::fs::write(&path, "copy:\n  archive_template: \"{year/{month}}\"\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.archive_template, "{year}/{month}/{valuable_name}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 非法 log.level 回退 "info"，不让 CLI 端 parse 静默吞掉配置错误。
#[test]
fn load_sanitizes_invalid_log_level_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badlevel.yaml");
    std::fs::write(&path, "log:\n  level: chatty\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.log.level, "info");
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 合法 log.level 不被 sanitize 改写（防无条件重置变异）。
#[test]
fn load_keeps_valid_log_level_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oklevel.yaml");
    std::fs::write(&path, "log:\n  level: debug\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.log.level, "debug");
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 端到端回归：真实 config.yaml 写法（带引号 + 嵌套占位符默认值）必须
// 解析成功，不触发 parse_error 回退。
#[test]
fn load_parses_quoted_template_placeholder_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tmpl.yaml");
    std::fs::write(
        &path,
        "copy:\n  archive_template: \"${TIDYMEDIA_TEST_LOAD_TMPL:-{year}/{day}}\"\n  unique_name_max_attempts: 4\n",
    )
    .unwrap();
    remove_env_var("TIDYMEDIA_TEST_LOAD_TMPL");
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.archive_template, "{year}/{day}");
    // 同文件其余字段未因 parse_error 丢失
    assert_eq!(cfg.copy.unique_name_max_attempts, 4);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// OCR `binarize_threshold` 越界（≤0 或 ≥1 或 NaN）回退默认 0.3，避免恒真/恒假。
#[test]
fn load_sanitizes_invalid_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badbt.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    binarize_threshold: 1.5\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_zero_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zerobt.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    binarize_threshold: 0.0\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_nan_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nanbt.yaml");
    // YAML 1.1 `.nan` 是 NaN 字面量；is_finite() 路径分支
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    binarize_threshold: .nan\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_negative_ocr_min_text_pixel_ratio_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("negratio.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    min_text_pixel_ratio: -0.1\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_invalid_ocr_min_text_pixel_ratio_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badratio.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    min_text_pixel_ratio: 2.0\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_too_small_ocr_resize_max_side_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badside.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    resize_max_side: 32\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.backend.ocr.resize_max_side, 736);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_keeps_valid_ocr_fields_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("okocr.yaml");
    std::fs::write(
        &path,
        "backend:\n  ocr:\n    binarize_threshold: 0.4\n    min_text_pixel_ratio: 0.01\n    resize_max_side: 960\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert!((cfg.backend.ocr.binarize_threshold - 0.4).abs() < f32::EPSILON);
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.01).abs() < f32::EPSILON);
    assert_eq!(cfg.backend.ocr.resize_max_side, 960);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 合法配置不被 sanitize 改写（防 sanitize 被变异成无条件重置）。
#[test]
fn load_keeps_valid_copy_fields_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("valid.yaml");
    std::fs::write(
        &path,
        "copy:\n  unique_name_max_attempts: 3\n  archive_template: \"{year}/{day}\"\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load();
    assert_eq!(cfg.copy.unique_name_max_attempts, 3);
    assert_eq!(cfg.copy.archive_template, "{year}/{day}");
    remove_env_var("TIDYMEDIA_CONFIG");
}
