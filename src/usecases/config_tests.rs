use super::{Config, chrono_offset_from_hours, offset_from_hours, validate_archive_template};
use time::UtcOffset;

#[test]
fn config_defaults_match_historical_constants() {
    let c = Config::default();
    assert_eq!(c.copy.timezone_offset_hours, 8);
    assert_eq!(c.copy.unique_name_max_attempts, 10);
    assert_eq!(c.copy.archive_template, "{year}/{month}/{valuable_name}");
    assert_eq!(c.copy.doc_archive_template, "{category}/{year}/{month}");
    assert_eq!(c.exif.valid_date_time_secs, 946_684_800);
    assert_eq!(c.backend.smb.default_user, "");
    assert_eq!(c.backend.smb.workgroup, "WORKGROUP");
    assert_eq!(c.backend.smb.timeout_secs, 30);
    assert_eq!(c.backend.adb.server_host, "127.0.0.1");
    assert_eq!(c.backend.adb.server_port, 5037);
    assert_eq!(c.backend.ocr.det_model_path, "");
    assert!((c.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    assert!((c.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    assert_eq!(c.backend.ocr.resize_max_side, 736);
    assert_eq!(c.backend.ocr.max_image_bytes, 50 * 1024 * 1024);
    assert_eq!(c.backend.face.scrfd_model_path, "");
    assert!((c.backend.face.scrfd_score_threshold - 0.5).abs() < f32::EPSILON);
    assert!((c.backend.face.scrfd_nms_iou - 0.4).abs() < f32::EPSILON);
    assert_eq!(c.backend.face.facenet_model_path, "");
    assert_eq!(c.backend.face.facemesh_model_path, "");
    assert_eq!(c.backend.face.eyestate_model_path, "");
    assert_eq!(c.backend.face.phash_hamming_max, 10);
    assert!((c.backend.face.sharpness_min - 100.0).abs() < f32::EPSILON);
    assert!((c.backend.face.face_cosine_min - 0.5).abs() < f32::EPSILON);
    assert!((c.backend.face.ear_blink_max - 0.21).abs() < f32::EPSILON);
    assert!((c.backend.face.eye_blink_score_max - 0.5).abs() < f32::EPSILON);
    assert!((c.backend.face.eye_crop_radius_ratio - 0.10).abs() < f32::EPSILON);
    assert!((c.backend.face.w_sharpness - 1.0).abs() < f32::EPSILON);
    assert!((c.backend.face.w_blink - 2.0).abs() < f32::EPSILON);
    assert!((c.backend.face.w_smile - 0.5).abs() < f32::EPSILON);
    assert_eq!(c.backend.face.max_image_bytes, 50 * 1024 * 1024);
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
