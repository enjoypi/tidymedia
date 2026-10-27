use super::{
    Config, chrono_offset_from_hours, config, install_loader, offset_from_hours, try_config,
    validate_archive_template,
};
use time::UtcOffset;

#[expect(clippy::unnecessary_wraps, reason = "匹配统一配置 loader 签名")]
fn debug_loader() -> Result<Config, String> {
    let mut cfg = Config::default();
    cfg.log.level = "debug".to_string();
    Ok(cfg)
}

fn failing_loader() -> Result<Config, String> {
    Err("boom".to_string())
}

#[test]
fn config_defaults_match_historical_constants() {
    let c = Config::default();
    assert_eq!(c.copy.timezone_offset_hours, 8);
    assert_eq!(c.copy.unique_name_max_attempts, 10);
    assert_eq!(c.copy.archive_template, "{year}/{month}/{valuable_name}");
    assert_eq!(c.copy.doc_archive_template, "{category}/{year}/{month}");
    assert_eq!(
        c.copy.skip_extensions,
        ["js", "json", "svg", "css", "ts", "html", "xml", "map"]
    );
    assert_eq!(c.exif.valid_date_time_secs, 946_684_800);
    assert_eq!(c.backend.smb.default_user, "");
    assert_eq!(c.backend.smb.workgroup, "WORKGROUP");
    assert_eq!(c.backend.smb.timeout_secs, 30);
    assert_eq!(c.backend.ocr.det_model_path, "");
    assert!((c.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    assert!((c.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    assert_eq!(c.backend.ocr.resize_max_side, 736);
    assert_eq!(c.backend.ocr.max_image_bytes, 50 * 1024 * 1024);
    assert_eq!(c.verify.max_image_bytes, 50 * 1024 * 1024);
    assert_eq!(c.backend.classify.embed_model_path, "");
    assert_eq!(c.backend.classify.tokenizer_path, "");
    assert!(c.backend.classify.categories.is_empty());
    assert!((c.backend.classify.score_min - 0.5).abs() < f32::EPSILON);
    assert_eq!(c.backend.classify.max_text_bytes, 4096);
    assert_eq!(c.log.level, "info");
}

#[test]
fn validate_archive_template_accepts_valid_template() {
    assert!(validate_archive_template("{year}/{month}/{day}").is_ok());
}

#[test]
fn validate_archive_template_rejects_empty() {
    assert!(validate_archive_template("").is_err());
}

#[test]
fn validate_archive_template_rejects_unbalanced_open() {
    let err = validate_archive_template("{year/{month}").unwrap_err();
    assert!(err.contains("unbalanced"), "got: {err}");
}

#[test]
fn validate_archive_template_rejects_unbalanced_close() {
    let err = validate_archive_template("year}/month").unwrap_err();
    assert!(err.contains("unbalanced"), "got: {err}");
}

// 计数配平但结构错配：旧字符计数实现会放过，渲染时产生字面 '{year' 目录。
#[test]
fn validate_archive_template_rejects_count_balanced_but_nested() {
    let err = validate_archive_template("{year/{month}}").unwrap_err();
    assert!(err.contains("nested"), "got: {err}");
}

#[test]
fn validate_archive_template_rejects_unclosed_open() {
    let err = validate_archive_template("{year").unwrap_err();
    assert!(err.contains("unclosed"), "got: {err}");
}

#[test]
fn validate_archive_template_rejects_unknown_placeholder() {
    let err = validate_archive_template("{year}/{foo}").unwrap_err();
    assert!(err.contains("unknown placeholder {foo}"), "got: {err}");
}

#[test]
fn validate_archive_template_accepts_all_known_placeholders() {
    assert!(
        validate_archive_template("{year}/{month}/{day}/{make}/{model}/{valuable_name}").is_ok()
    );
}

/// 仅 `{valuable_name}` 单占位符 → 渲染时 `valuable_name` 可能空 → 文件全落
/// output 根；validate 必须显式拒绝缺乏 always-non-empty 占位符的模板。
#[test]
fn validate_archive_template_rejects_template_without_safe_placeholder() {
    let err = validate_archive_template("{valuable_name}").unwrap_err();
    assert!(
        err.contains("at least one of"),
        "expected guidance about safe placeholders, got: {err}"
    );
}

/// 单 always-non-empty 占位符即可通过（year 已保证非空）。
#[test]
fn validate_archive_template_accepts_single_safe_placeholder() {
    assert!(validate_archive_template("{year}").is_ok());
}

/// 纯静态文本无占位符 → 渲染恒同字面量，无年/月分桶，同样拒绝。
#[test]
fn validate_archive_template_rejects_pure_static_text() {
    let err = validate_archive_template("archive").unwrap_err();
    assert!(err.contains("at least one of"), "got: {err}");
}

#[test]
fn offset_from_hours_valid_value_produces_expected_offset() {
    assert_eq!(offset_from_hours(8).whole_seconds(), 8 * 3600);
}

#[test]
fn offset_from_hours_out_of_range_falls_back_to_utc() {
    assert_eq!(offset_from_hours(127), UtcOffset::UTC);
}

#[test]
fn chrono_offset_from_hours_valid() {
    let off = chrono_offset_from_hours(8);
    assert_eq!(off.local_minus_utc(), 8 * 3600);
}

#[test]
fn chrono_offset_from_hours_out_of_range_falls_back_to_utc() {
    assert_eq!(chrono_offset_from_hours(25).local_minus_utc(), 0);
}

#[test]
fn config_rejects_unknown_top_level_field() {
    let err = serde_yaml::from_str::<Config>("legacy: true\n").unwrap_err();
    assert!(format!("{err}").contains("legacy"), "got: {err}");
}

#[test]
fn config_rejects_unknown_nested_field() {
    let err = serde_yaml::from_str::<Config>("backend:\n  smb:\n    timeout: 3\n").unwrap_err();
    assert!(format!("{err}").contains("timeout"), "got: {err}");
}

#[test]
fn config_allows_partial_yaml_with_known_fields() {
    let cfg: Config = serde_yaml::from_str("copy:\n  unique_name_max_attempts: 3\n").unwrap();
    assert_eq!(cfg.copy.unique_name_max_attempts, 3);
    assert_eq!(cfg.log.level, "info");
}

#[test]
fn try_config_defaults_without_loader() {
    let cfg = try_config().unwrap();
    assert_eq!(cfg.log.level, "info");
}

#[test]
fn try_config_caches_ok_result() {
    install_loader(debug_loader);
    let a = try_config().unwrap();
    let b = try_config().unwrap();
    assert!(std::ptr::eq(a, b));
    assert_eq!(a.log.level, "debug");
}

#[test]
fn try_config_caches_error_result() {
    install_loader(failing_loader);
    assert_eq!(try_config().unwrap_err(), "boom");
    assert_eq!(try_config().unwrap_err(), "boom");
}

#[test]
#[should_panic(expected = "internal: config() requires successful try_config() precheck")]
fn config_panics_when_load_failed() {
    install_loader(failing_loader);
    let _ = config();
}
