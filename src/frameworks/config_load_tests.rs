use super::load;
use super::test_common::remove_env_var;
use super::test_common::set_env_var;
use crate::usecases::config::config;

#[test]
fn load_errors_on_unknown_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backend.yaml");
    std::fs::write(
        &path,
        "backend:\n  smb:\n    default_user: alice\n    workgroup: HOME\n  legacy:\n    enabled: false\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let err = load().unwrap_err();
    assert!(err.contains("legacy"), "got: {err}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_errors_when_explicit_file_missing() {
    set_env_var("TIDYMEDIA_CONFIG", "/no/such/file/xyz.yaml");
    let err = load().unwrap_err();
    assert!(err.contains("/no/such/file/xyz.yaml"), "got: {err}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_errors_when_yaml_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.yaml");
    std::fs::write(&path, "::: not yaml :::").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let err = load().unwrap_err();
    assert!(err.contains("cannot parse config file"), "got: {err}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_errors_on_unreadable_config_path() {
    let dir = tempfile::tempdir().unwrap();
    set_env_var("TIDYMEDIA_CONFIG", dir.path().to_str().unwrap());
    let err = load().unwrap_err();
    assert!(err.contains("cannot read config file"), "got: {err}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_uses_defaults_when_default_file_missing() {
    remove_env_var("TIDYMEDIA_CONFIG");
    let dir = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let cfg = load().unwrap();
    std::env::set_current_dir(original).unwrap();
    assert_eq!(cfg.copy.timezone_offset_hours, 8);
}

#[test]
fn load_errors_when_default_file_corrupt() {
    remove_env_var("TIDYMEDIA_CONFIG");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.yaml"), "::: not yaml :::").unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let res = load();
    std::env::set_current_dir(original).unwrap();
    assert!(res.unwrap_err().contains("cannot parse config file"));
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
    assert_eq!(cfg.copy.unique_name_max_attempts, 10);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_normalizes_skip_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ext.yaml");
    std::fs::write(
        &path,
        "copy:\n  skip_extensions: [\".JS\", \" Json \", \"\"]\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.copy.skip_extensions, ["js", "json"]);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// 0 让 smb2 每次请求立即超时：sanitize 必须 warn + 回退默认 30。
#[test]
fn load_sanitizes_zero_smb_timeout_secs_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zerosmb.yaml");
    std::fs::write(&path, "backend:\n  smb:\n    timeout_secs: 0\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.backend.smb.timeout_secs, 30);
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
    assert_eq!(cfg.copy.archive_template, "{year}/{day}");
    // 同文件其余字段未因 parse_error 丢失
    assert_eq!(cfg.copy.unique_name_max_attempts, 4);
    remove_env_var("TIDYMEDIA_CONFIG");
}

// OCR `binarize_threshold` 越界（≤0 或 ≥1 或 NaN）回退默认 0.3，避免恒真/恒假。

include!("config_load_more_tests.rs");
