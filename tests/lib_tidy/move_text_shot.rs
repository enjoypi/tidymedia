//! `move-text-shot` 子命令端到端集成测试。
//!
//! 覆盖维度（tract-onnx + image 默认编译，无 feature gate）：
//! - model path 空：`dispatch_move_text_shot` 在 `build_detector` 返 `InvalidInput`
//! - model path 指向不存在文件：`build_detector` 返 Ok（懒加载），walk 到首张
//!   image 时触发 `load_runnable` Err（`_real.rs` 排除文件）→ `report.failed` 累加
//! - `--report` 写盘：dispatch 的 `if let Some(report_path)` Some 侧
//! - source ⊆ output：usecase 返 `InvalidInput`，dispatch `?` 传播
//! - partial-failure：`report.failed > 0` 让 `tidy()` 返 Err 含 "move-text-shot partial failure"
//!
//! Lib unit 测试已覆盖 usecase 内部所有分支（`src/usecases/move_text_shot/run_tests.rs`），
//! 集成测试只验 dispatch 与 CLI 字符串映射这两条 e2e 链路。

use std::fs;

use tempfile::tempdir;
use tidymedia::{Commands, tidy};

use super::local;

/// 把 OCR 配置临时切到指定 model path：`dispatch_move_text_shot` 读全局 config，
/// 这里通过 env var + `TIDYMEDIA_CONFIG` 切独立 config.yaml。
/// 注意：全局 `OnceLock<Config>` 一旦初始化就不能改，所以**单个测试进程内
/// 只能跑一次**。nextest 默认每测试独立进程，自动满足。
fn write_temp_config(model_path: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    let yaml = format!(
        "backend:\n  ocr:\n    det_model_path: \"{model_path}\"\n    binarize_threshold: 0.3\n    min_text_pixel_ratio: 0.005\n    resize_max_side: 736\n"
    );
    fs::write(&cfg_path, yaml).unwrap();
    // SAFETY: nextest 每测试独立进程，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
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

/// `build_detector` OK 路径 + 写 report 路径 + tidy 成功路径：non-existent ONNX
/// 让 `build_detector` 返 Ok（懒加载，构造时不读文件），空 source 让 usecase
/// 直接返 `Ok(report{ failed=0 })`，tidy 返 Ok。覆盖 `dispatch_move_text_shot`
/// 的 `Some(report_path)` 写盘分支。
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
    assert!(contents.contains("\"scanned\": 0"), "got: {contents}");
    assert!(contents.contains("\"moved\": 0"), "got: {contents}");
}

/// dispatch 内 `move_text_shot(...)?` Err arm：source ⊆ output 让 usecase 返 Err。
/// 配合 `dispatch_returns_invalid_input_when_model_path_empty`（`build_detector` Err）
/// 覆盖 dispatch 中两个不同的 `?` Err arm。
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

/// `tidy()` 的 `MoveTextShot` partial-failure arm：源含 PNG → `has_text` 触发
/// `load_runnable` Err（model 文件不存在） → `report.failed=1` → `tidy` 返 Err
/// 含 "move-text-shot partial failure"。
#[test]
fn tidy_returns_err_when_move_text_shot_partial_failure() {
    let _cfg = write_temp_config("/nonexistent/det.onnx");
    let src = tempdir().unwrap();
    // PNG 8-byte signature 即足以让 infer 识别 image/png
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
