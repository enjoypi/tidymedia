//! `archive_template` 验证 + `--report` 写盘的 dispatch 路径测试。
//! 从 `tests/lib_tidy.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::sync::Arc;

use tempfile::tempdir;
use tidymedia::{Backend, Commands, FakeBackend, tidy, tidy_with};

use super::{DATA_DIR, FakeBackendFactory, adb_loc, local, smb_loc};

// Find + report：触发 dispatch.rs L56 `if let Some(path)` True 分支（find report 写盘）。
#[test]
fn tidy_dispatches_find_with_report_writes_json() {
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("find_report.json");
    tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: None,
        report: Some(report_path.to_str().unwrap().to_string()),
    })
    .expect("find with report should succeed");
    // 报告文件存在且是合法 JSON。
    let content = std::fs::read_to_string(&report_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["scanned"].as_u64().is_some());
}

// Copy + archive_template：触发 dispatch.rs L103 validate_template_arg Some 分支。
#[test]
fn tidy_dispatches_copy_with_archive_template_validates() {
    let out = tempdir().unwrap();
    tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year}/{month}/{day}".to_string()),
        report: None,
    })
    .expect("copy with valid archive_template should succeed");
}

// Move + archive_template：同样覆盖 Move 分支的 validate_template_arg Some path。
#[test]
fn tidy_dispatches_move_with_archive_template_validates() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year}/{month}".to_string()),
        report: None,
    })
    .expect("move with valid archive_template should succeed");
}

// Copy + 非法 archive_template → dispatch 返 Err（validate_template_arg 的 `?` Err 分支）。
#[test]
fn tidy_rejects_copy_with_invalid_archive_template() {
    let out = tempdir().unwrap();
    let err = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year/{month}".to_string()), // unbalanced brace
        report: None,
    })
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

// Move + 非法 archive_template → dispatch 返 Err（Move 分支 validate_template_arg `?`）。
#[test]
fn tidy_rejects_move_with_invalid_archive_template() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let err = tidy(Commands::Move {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("year}".to_string()), // extra closing brace
        report: None,
    })
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid --archive-template"), "got: {msg}");
}

// archive_template 端到端写盘：dry_run=false + `{year}/{month}/{day}` 模板 →
// sample-with-offset.jpg 携带 EXIF `DateTimeOriginal=2024:05:01 14:30:00 +08:00`，
// `copy.rs::do_copy` 经 `Info::create_time`（EXIF P0）取值后转 local（默认 timezone=+8）→
// 文件实际落到 output/2024/05/01/<某文件名>。
#[test]
fn tidy_dispatches_copy_with_archive_template_writes_year_month_day_layout() {
    let src_dir = tempdir().unwrap();
    // 用 tests/data/sample-with-offset.jpg 作种子复制到 tempdir 避免污染 fixture
    // （sample-with-offset.jpg 含 DateTimeOriginal=2024:05:01 14:30 +08:00）。
    let src_file = src_dir.path().join("sample-with-offset.jpg");
    std::fs::copy(format!("{DATA_DIR}/sample-with-offset.jpg"), &src_file)
        .expect("copy fixture into tempdir");

    let out = tempdir().unwrap();
    tidy(Commands::Copy {
        dry_run: false,
        include_non_media: false,
        sources: vec![local(src_dir.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year}/{month}/{day}".to_string()),
        report: None,
    })
    .expect("copy with archive_template should succeed");

    // 三级目录应当存在：output/2024/05/01/<...>
    let day_dir = out.path().join("2024").join("05").join("01");
    assert!(
        day_dir.is_dir(),
        "expected {} to be a directory after copy with {{year}}/{{month}}/{{day}} template",
        day_dir.display()
    );
    let files: Vec<_> = std::fs::read_dir(&day_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .collect();
    assert!(
        !files.is_empty(),
        "expected at least one file under {}",
        day_dir.display()
    );
}

/// ADB source → SMB output：手机照片直接归档到 NAS，全程走 `FakeBackend` 不需真实设备/服务器。
#[test]
fn tidy_with_copy_adb_source_to_smb_output() {
    let adb_root = adb_loc("/sdcard/DCIM");
    let adb_file = adb_loc("/sdcard/DCIM/shot.jpg");

    let fake_adb = Arc::new(FakeBackend::new("adb"));
    fake_adb.add_dir(adb_root.clone());
    fake_adb.add_file(adb_file, b"RAW-PHOTO-ADB".to_vec());

    let smb_root = smb_loc("Inbox");
    let fake_smb = Arc::new(FakeBackend::new("smb"));
    fake_smb.add_dir(smb_root.clone());

    let mut factory = FakeBackendFactory::new();
    factory.insert("adb", Arc::clone(&fake_adb) as Arc<dyn Backend>);
    factory.insert("smb", Arc::clone(&fake_smb) as Arc<dyn Backend>);

    tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: true,
            sources: vec![adb_root],
            output: smb_root,
            archive_template: None,
            report: None,
        },
    )
    .expect("adb -> smb copy should succeed");

    // FakeBackend metadata 给 UNIX_EPOCH → 1970/01 桶。
    let dst_loc = smb_loc("Inbox/1970/01/shot.jpg");
    assert!(
        fake_smb.read_bytes(&dst_loc).is_some(),
        "expected smb to hold copied file at Inbox/1970/01/shot.jpg"
    );
}
