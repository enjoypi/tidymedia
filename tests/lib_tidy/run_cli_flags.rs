//! `run_cli` 字符串参数 e2e：补齐每个 flag 在 copy/move/find 路径上的 Some 侧
//! 与 `dry_run=false` 真实执行（`dispatch_and_cli.rs` 的 `run_cli` 系列全是 --dry-run）。
//! fixture 统一用 `sample-with-offset.jpg`（EXIF `DateTimeOriginal=2024-05-01 +08:00`，
//! 不依赖 mtime；P2 文件名 fixture 不被 `Info::create_time` 消费）。

use std::path::Path;

use tempfile::tempdir;
use tidymedia::run_cli;

use super::DATA_DIR;

const EXIF_FIXTURE: &str = "sample-with-offset.jpg";

fn seed_fixture(dir: &Path) -> std::path::PathBuf {
    let dst = dir.join(EXIF_FIXTURE);
    std::fs::copy(format!("{DATA_DIR}/{EXIF_FIXTURE}"), &dst).expect("seed fixture into tempdir");
    dst
}

fn file_count_recursive(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                file_count_recursive(&p)
            } else {
                1
            }
        })
        .sum()
}

#[test]
fn run_cli_copy_real_run_writes_output() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy without --dry-run via run_cli should succeed");
    assert!(
        file_count_recursive(out.path()) > 0,
        "real copy must write at least one file under output"
    );
}

#[test]
fn run_cli_move_real_run_removes_source_and_writes_output() {
    let src = tempdir().unwrap();
    let src_file = seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move without --dry-run via run_cli should succeed");
    assert!(!src_file.exists(), "real move must remove the source file");
    assert!(
        file_count_recursive(out.path()) > 0,
        "real move must write at least one file under output"
    );
}

#[test]
fn run_cli_copy_archive_template_writes_year_month_day_layout() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{year}/{month}/{day}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy --archive-template via run_cli should succeed");
    let day_dir = out.path().join("2024").join("05").join("01");
    assert!(
        file_count_recursive(&day_dir) > 0,
        "expected file under {}",
        day_dir.display()
    );
}

#[test]
fn run_cli_copy_rejects_invalid_archive_template() {
    let out = tempdir().unwrap();
    let err = run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{year/{month}", // unbalanced brace
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

#[test]
fn run_cli_move_archive_template_writes_year_month_layout() {
    let src = tempdir().unwrap();
    let src_file = seed_fixture(src.path());
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move",
        "--archive-template",
        "{year}/{month}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --archive-template via run_cli should succeed");
    assert!(!src_file.exists(), "move must remove the source file");
    let month_dir = out.path().join("2024").join("05");
    assert!(
        file_count_recursive(&month_dir) > 0,
        "expected file under {}",
        month_dir.display()
    );
}

#[test]
fn run_cli_move_rejects_invalid_archive_template() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let err = run_cli([
        "tidymedia",
        "move",
        "--archive-template",
        "year}", // extra closing brace
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

#[test]
fn run_cli_copy_report_writes_json() {
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("copy_report.json");
    run_cli([
        "tidymedia",
        "copy",
        "--dry-run",
        "--report",
        report_path.to_str().unwrap(),
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("copy --report via run_cli should succeed");
    let content = std::fs::read_to_string(&report_path).expect("report file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("report must be JSON");
    assert!(parsed["scanned"].as_u64().is_some());
}

#[test]
fn run_cli_move_report_writes_json() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("move_report.json");
    run_cli([
        "tidymedia",
        "move",
        "--dry-run",
        "--report",
        report_path.to_str().unwrap(),
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("move --report via run_cli should succeed");
    assert!(report_path.exists(), "report should be written");
}

#[test]
fn run_cli_find_report_writes_json() {
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("find_report.json");
    run_cli([
        "tidymedia",
        "find",
        "--report",
        report_path.to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("find --report via run_cli should succeed");
    let content = std::fs::read_to_string(&report_path).expect("report file must exist");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("report must be JSON");
    assert!(parsed["scanned"].as_u64().is_some());
}

#[test]
fn run_cli_find_with_output_dispatches() {
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "find",
        "--output",
        out.path().to_str().unwrap(),
        DATA_DIR,
    ])
    .expect("find --output via run_cli should succeed");
}

/// 已知子命令但缺必填 `--output` → clap 解析失败走 `run_cli` 的 Err 分支
/// （区别于既有的"未知子命令"用例）。
#[test]
fn run_cli_copy_missing_required_output_returns_err() {
    let err = run_cli(["tidymedia", "copy", DATA_DIR]).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("--output"), "got: {msg}");
}

/// spawn 真实 binary 验证 `install_logging` 真装上了 subscriber：`--log-level debug`
/// 时 `run_cli` 的 "cli parsed" debug 日志必须落在 stderr。in-process 测不了——
/// 全局 subscriber 只能 `try_init` 一次；且变异 `install_logging → ()` 时唯一可观测
/// 差异就是 stderr 没有日志输出。
#[test]
fn binary_emits_debug_log_to_stderr_when_log_level_debug() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args(["--log-level", "debug", "find", src.path().to_str().unwrap()])
        .env_remove("RUST_LOG") // 不让外部 RUST_LOG 干扰 EnvFilter::try_from_default_env
        .output()
        .expect("spawn tidymedia binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stderr.contains("cli parsed"),
        "binary must log 'cli parsed' to stderr at debug level; status: {:?}, stderr: {stderr}",
        out.status
    );
}

/// 不带 `--log-level` 时 binary 从 config.yaml `log.level` 取级别：配置 debug
/// → "cli parsed" debug 日志必须落 stderr。杀 `install_logging` 中
/// `unwrap_or_else(config_level)` None 侧接线类变异（默认 info 时该日志不输出）。
#[test]
fn binary_uses_config_log_level_when_flag_absent() {
    let src = tempdir().unwrap();
    seed_fixture(src.path());
    let cfg_dir = tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(&cfg_path, "log:\n  level: debug\n").expect("write test config");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args(["find", src.path().to_str().unwrap()])
        .env_remove("RUST_LOG") // 不让外部 RUST_LOG 干扰 EnvFilter::try_from_default_env
        .env("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap())
        .output()
        .expect("spawn tidymedia binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stderr.contains("cli parsed"),
        "binary must honor config log.level=debug; status: {:?}, stderr: {stderr}",
        out.status
    );
}

/// e2e 走真实 copy 路径验证 `Info::exif_ref` 接线：`{make}/{model}` 必须渲染出
/// fixture 的真值目录。`exif_ref` 被变异成 `None` 或 `Some(默认 Exif)` 时
/// 两个占位符都退化为 "unknown"，本断言即失败。
#[test]
fn run_cli_copy_archive_template_renders_make_model_from_exif() {
    let src = tempdir().unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-make-model.jpg"),
        src.path().join("m.jpg"),
    )
    .expect("seed make/model fixture");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--archive-template",
        "{make}/{model}",
        "-o",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy with make/model template should succeed");
    assert!(
        out.path().join("TestCam/TestModel/m.jpg").exists(),
        "make/model from real EXIF must shape the archive dir"
    );
}

/// AVI（RIFF strd 内嵌 EXIF）e2e：nom-exif 不支持 RIFF，`entities::riff` 自解析
/// 提供 P0 候选——fixture 内嵌 "2005:04:26 20:10:00"（默认 +8 时区）必须归档到
/// 2005/04，而非 mtime 年月（checkout 时间）。杀 `from_reader` AVI 分流接线变异。
#[test]
fn run_cli_copy_avi_archives_by_embedded_exif() {
    let src = tempdir().unwrap();
    let avi = src.path().join("sample-fuji-strd.avi");
    std::fs::copy(format!("{DATA_DIR}/sample-fuji-strd.avi"), &avi).expect("seed AVI fixture");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("copy AVI via run_cli should succeed");
    assert!(
        out.path()
            .join("2005")
            .join("04")
            .join("sample-fuji-strd.avi")
            .exists(),
        "AVI must be archived by embedded EXIF time 2005/04"
    );
}

/// spawn binary 断言 find -o 的脚本语义：output 目录外的重复文件出 `rm` 行、
/// output 目录内的被注释保留。杀 `compute_output_prefix -> None`（变异后全部
/// 被注释，stdout 无未注释 rm 行）及 `under_prefix` 边界类变异。
#[test]
fn binary_find_with_output_emits_rm_for_outside_and_comment_for_inside() {
    let root = tempdir().unwrap();
    let keep = root.path().join("keep");
    std::fs::create_dir(&keep).unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-offset.jpg"),
        keep.join("a.jpg"),
    )
    .expect("seed kept copy");
    std::fs::copy(
        format!("{DATA_DIR}/sample-with-offset.jpg"),
        root.path().join("b.jpg"),
    )
    .expect("seed duplicate outside output");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidymedia"))
        .args([
            "find",
            root.path().to_str().unwrap(),
            "-o",
            keep.to_str().unwrap(),
        ])
        .output()
        .expect("spawn tidymedia binary");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 删除命令按平台渲染：Windows 输出 `DEL ...`（`:` 注释），Unix 输出 `rm ...`（`#` 注释）
    let rm_prefix = if cfg!(target_os = "windows") {
        "DEL "
    } else {
        "rm "
    };
    let rm_outside = stdout
        .lines()
        .any(|l| l.starts_with(rm_prefix) && l.contains("b.jpg"));
    let kept_commented = stdout
        .lines()
        .filter(|l| l.contains("a.jpg"))
        .all(|l| !l.starts_with(rm_prefix));
    assert!(
        rm_outside && kept_commented,
        "outside dup must be rm'd, kept copy commented; stdout:\n{stdout}"
    );
}
