//! `cull` 子命令端到端集成测试。覆盖：CLI clap 注册 / 4 build_* 路径校验 /
//! source ⊆ output 保护 / dry-run / partial-failure / 真搬迁产物。
//!
//! 4 detector 真实 ONNX 模型 CI 不可触发——所有"流水线行为"测试用 PNG 字节 +
//! 不存在的 model path 触发 `load_runnable` Err 路径，验 report.failed 计数。

use std::fs;

use tempfile::tempdir;
use tidymedia::{Commands, run_cli, tidy};

use super::local;

/// 把 face 配置临时切到指定 4 个 model path。`TIDYMEDIA_CONFIG` env 切独立 yaml。
/// 全局 `OnceLock` 一旦初始化不能改，故单测试进程内只跑一次（nextest 默认满足）。
fn write_temp_config(
    scrfd: &str,
    facenet: &str,
    facemesh: &str,
    eyestate: &str,
) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    let yaml = format!(
        "backend:\n  face:\n    scrfd_model_path: \"{scrfd}\"\n    facenet_model_path: \"{facenet}\"\n    facemesh_model_path: \"{facemesh}\"\n    eyestate_model_path: \"{eyestate}\"\n    phash_hamming_max: 10\n    sharpness_min: 100.0\n    face_cosine_min: 0.5\n    ear_blink_max: 0.21\n    eye_blink_score_max: 0.5\n    w_sharpness: 1.0\n    w_blink: 2.0\n    w_smile: 0.5\n"
    );
    fs::write(&cfg_path, yaml).unwrap();
    // SAFETY: nextest 每测试独立进程，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    dir
}

#[test]
fn run_cli_cull_help_succeeds() {
    // clap `--help` 走 DisplayHelp 分支返 Ok；同时验子命令注册
    let result = run_cli(["tidymedia", "cull", "--help"]);
    assert!(result.is_ok(), "got: {result:?}");
}

#[test]
fn dispatch_returns_invalid_input_when_scrfd_path_empty() {
    let _cfg = write_temp_config("", "/tmp/m2", "/tmp/m3", "/tmp/m4");
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let err = tidy(Commands::Cull {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        phash_max: None,
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("scrfd_model_path is empty"), "got: {msg}");
}

#[test]
fn dispatch_ok_path_writes_report_when_source_has_no_image() {
    let _cfg = write_temp_config(
        "/nonexistent/scrfd.onnx",
        "/nonexistent/facenet.onnx",
        "/nonexistent/facemesh.onnx",
        "/nonexistent/eyestate.onnx",
    );
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_path = out.path().join("cull-report.json");
    tidy(Commands::Cull {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        phash_max: None,
        report: Some(report_path.to_str().unwrap().to_string()),
    })
    .expect("空 source → 不调 detector，build_* 懒加载不报错");
    let contents = fs::read_to_string(&report_path).expect("report written");
    assert!(contents.contains("\"scanned\": 0"), "got: {contents}");
    assert!(contents.contains("\"grouped\": 0"), "got: {contents}");
}

#[test]
fn dispatch_propagates_usecase_error_when_source_inside_output() {
    let _cfg = write_temp_config(
        "/nonexistent/scrfd.onnx",
        "/nonexistent/facenet.onnx",
        "/nonexistent/facemesh.onnx",
        "/nonexistent/eyestate.onnx",
    );
    let dir = tempdir().unwrap();
    let src = dir.path().join("a");
    fs::create_dir_all(&src).unwrap();
    let err = tidy(Commands::Cull {
        dry_run: true,
        sources: vec![local(src.to_str().unwrap())],
        output: local(dir.path().to_str().unwrap()),
        phash_max: None,
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("is inside output"), "got: {msg}");
}

/// 两张相同 PNG → ahash 相同 → 同组 → 触发 SCRFD `load_runnable` Err → `report.failed`
/// → `tidy()` 走 partial-failure arm。
#[test]
fn tidy_returns_err_when_cull_partial_failure() {
    use image::ImageEncoder;
    let _cfg = write_temp_config(
        "/nonexistent/scrfd.onnx",
        "/nonexistent/facenet.onnx",
        "/nonexistent/facemesh.onnx",
        "/nonexistent/eyestate.onnx",
    );
    let src = tempdir().unwrap();
    // 写两张 16×16 PNG（同色 → 同 ahash）
    for name in &["a.png", "b.png"] {
        let mut buf = Vec::new();
        let pixels = vec![128_u8; 16 * 16 * 3];
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(&pixels, 16, 16, image::ExtendedColorType::Rgb8)
            .unwrap();
        fs::write(src.path().join(name), buf).unwrap();
    }
    let out = tempdir().unwrap();
    let err = tidy(Commands::Cull {
        dry_run: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        phash_max: None,
        report: None,
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cull partial failure"), "got: {msg}");
}

#[test]
fn run_cli_cull_string_form_accepts_phash_max_flag() {
    // 字符串形式验 clap 名映射（--phash-max → phash_max: Option<u8>）
    // 配置缺失 → InvalidInput；但只要 clap 解析成功就证明 flag 注册
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    // 不切 config → 默认 face.scrfd_model_path = "" → build_scrfd_detector InvalidInput
    let err = run_cli([
        "tidymedia",
        "cull",
        "--dry-run",
        "--phash-max",
        "5",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .unwrap_err();
    let msg = err.to_string();
    // clap 解析成功 → 进入 dispatch_cull → build_scrfd_detector 空路径 InvalidInput
    assert!(msg.contains("scrfd_model_path is empty"), "got: {msg}");
}
