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
    // 装 yaml loader 让 lib API 内部 config() 走自定义 yaml 而非 default
    tidymedia::install_config_loader();
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
fn dispatch_returns_invalid_input_when_facenet_path_empty() {
    let _cfg = write_temp_config("/tmp/m1", "", "/tmp/m3", "/tmp/m4");
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
    assert!(msg.contains("facenet_model_path is empty"), "got: {msg}");
}

#[test]
fn dispatch_returns_invalid_input_when_facemesh_path_empty() {
    let _cfg = write_temp_config("/tmp/m1", "/tmp/m2", "", "/tmp/m4");
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
    assert!(msg.contains("facemesh_model_path is empty"), "got: {msg}");
}

#[test]
fn dispatch_returns_invalid_input_when_eyestate_path_empty() {
    let _cfg = write_temp_config("/tmp/m1", "/tmp/m2", "/tmp/m3", "");
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
    assert!(msg.contains("eyestate_model_path is empty"), "got: {msg}");
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
fn dispatch_non_dry_run_creates_output_dir() {
    // 让 lib_tidy 集成 instance 也走 dry_run=false 路径，覆盖 cull 的 if !dry_run 分支
    let _cfg = write_temp_config(
        "/nonexistent/scrfd.onnx",
        "/nonexistent/facenet.onnx",
        "/nonexistent/facemesh.onnx",
        "/nonexistent/eyestate.onnx",
    );
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    tidy(Commands::Cull {
        dry_run: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        phash_max: None,
        report: None,
    })
    .expect("空 source + 非 dry-run → mkdir_p 成功，不调 detector");
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
    // 写两张 64×64 噪声 PNG（同 seed pattern → 同 phash 入同组；高 laplacian variance
    // 保证 sharpness > sharpness_min=100 通过 filter_blurry，否则 grouped=0 不触发
    // SCRFD load_runnable Err 无法到达 partial-failure arm）。
    for name in &["a.png", "b.png"] {
        let mut buf = Vec::new();
        let mut pixels = Vec::with_capacity(64 * 64 * 3);
        for i in 0_u32..(64 * 64) {
            let v = i.wrapping_mul(37) ^ (i >> 3);
            let noise = (v & 0xff) as u8;
            pixels.extend_from_slice(&[noise, noise, noise]);
        }
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(&pixels, 64, 64, image::ExtendedColorType::Rgb8)
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
    // 切空 scrfd 路径强制 build_scrfd_detector InvalidInput；只要 clap 解析成功
    // 报错文案含 "scrfd_model_path is empty" 即证 flag 注册 + dispatch 链路通
    let _cfg = write_temp_config("", "/tmp/m2", "/tmp/m3", "/tmp/m4");
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
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
    assert!(msg.contains("scrfd_model_path is empty"), "got: {msg}");
}
