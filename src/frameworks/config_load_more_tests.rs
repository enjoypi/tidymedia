#[test]
fn load_sanitizes_invalid_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badbt.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    binarize_threshold: 1.5\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_zero_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zerobt.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    binarize_threshold: 0.0\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_nan_ocr_binarize_threshold_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nanbt.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    binarize_threshold: .nan\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_negative_ocr_min_text_pixel_ratio_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("negratio.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    min_text_pixel_ratio: -0.1\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_invalid_ocr_min_text_pixel_ratio_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badratio.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    min_text_pixel_ratio: 2.0\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_too_small_ocr_resize_max_side_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badside.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    resize_max_side: 32\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
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
    let cfg = load().unwrap();
    assert!((cfg.backend.ocr.binarize_threshold - 0.4).abs() < f32::EPSILON);
    assert!((cfg.backend.ocr.min_text_pixel_ratio - 0.01).abs() < f32::EPSILON);
    assert_eq!(cfg.backend.ocr.resize_max_side, 960);
    remove_env_var("TIDYMEDIA_CONFIG");
}

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
    let cfg = load().unwrap();
    assert_eq!(cfg.copy.unique_name_max_attempts, 3);
    assert_eq!(cfg.copy.archive_template, "{year}/{day}");
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_undersized_max_image_bytes_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify_max_bytes_small.yaml");
    std::fs::write(&path, "verify:\n  max_image_bytes: 1024\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.verify.max_image_bytes, 50 * 1024 * 1024);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_midrange_max_image_bytes_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify_max_bytes_mid.yaml");
    std::fs::write(&path, "verify:\n  max_image_bytes: 5000\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.verify.max_image_bytes, 50 * 1024 * 1024);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_undersized_ocr_max_image_bytes_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ocr_max_bytes_small.yaml");
    std::fs::write(&path, "backend:\n  ocr:\n    max_image_bytes: 512\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.backend.ocr.max_image_bytes, 50 * 1024 * 1024);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_keeps_valid_verify_max_image_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify_ok.yaml");
    std::fs::write(&path, "verify:\n  max_image_bytes: 2097152\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.verify.max_image_bytes, 2 * 1024 * 1024);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_invalid_classify_score_min_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("badscore.yaml");
    std::fs::write(&path, "backend:\n  classify:\n    score_min: 1.5\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert!((cfg.backend.classify.score_min - 0.5).abs() < f32::EPSILON);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_sanitizes_zero_classify_max_text_bytes_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zerotext.yaml");
    std::fs::write(&path, "backend:\n  classify:\n    max_text_bytes: 0\n").unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.backend.classify.max_text_bytes, 4096);
    remove_env_var("TIDYMEDIA_CONFIG");
}

#[test]
fn load_parses_classify_categories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cats.yaml");
    std::fs::write(
        &path,
        "backend:\n  classify:\n    categories:\n      - {name: invoice, description: \"发票\"}\n",
    )
    .unwrap();
    set_env_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
    let cfg = load().unwrap();
    assert_eq!(cfg.backend.classify.categories.len(), 1);
    assert_eq!(cfg.backend.classify.categories[0].name, "invoice");
    assert_eq!(cfg.backend.classify.categories[0].description, "发票");
    remove_env_var("TIDYMEDIA_CONFIG");
}
