use std::fs;

use tempfile::tempdir;
use tidymedia::{Commands, tidy};

use super::local;

fn write_temp_config(model_path: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    let yaml = format!(
        "backend:\n  ocr:\n    det_model_path: \"{model_path}\"\n    binarize_threshold: 0.3\n    min_text_pixel_ratio: 0.005\n    resize_max_side: 736\n"
    );
    fs::write(&cfg_path, yaml).unwrap();
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    tidymedia::install_config_loader();
    dir
}

#[test]
fn dispatch_returns_invalid_input_when_model_path_empty() {
    let _cfg = write_temp_config("");
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let err = tidy(Commands::MoveTextShot {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("det_model_path is empty"), "got: {msg}");
}

#[test]
fn dispatch_ok_path_writes_report_when_source_has_no_image() {
    let _cfg = write_temp_config("/nonexistent/det.onnx");
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_path = out.path().join("report.json");
    tidy(Commands::MoveTextShot {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        report: Some(report_path.to_str().unwrap().to_string()),
    })
    .expect("空 source 即无 image 调用，build_detector 懒加载不报错");
    let contents = fs::read_to_string(&report_path).expect("report should be written");
    assert!(contents.contains("\"image_files\": 0"), "got: {contents}");
    assert!(contents.contains("\"moved\": 0"), "got: {contents}");
}

#[test]
fn dispatch_propagates_usecase_error_when_source_inside_output() {
    let _cfg = write_temp_config("/nonexistent/det.onnx");
    let dir = tempdir().unwrap();
    let src = dir.path().join("a");
    fs::create_dir_all(&src).unwrap();
    let err = tidy(Commands::MoveTextShot {
        dry_run: true,
        sources: vec![local(src.to_str().unwrap())],
        output: local(dir.path().to_str().unwrap()),
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("is inside output"), "got: {msg}");
}

#[test]
fn tidy_returns_err_when_move_text_shot_partial_failure() {
    let _cfg = write_temp_config("/nonexistent/det.onnx");
    let src = tempdir().unwrap();
    fs::write(
        src.path().join("a.png"),
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    )
    .unwrap();
    let out = tempdir().unwrap();
    let err = tidy(Commands::MoveTextShot {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("move-text-shot partial failure"), "got: {msg}");
}
