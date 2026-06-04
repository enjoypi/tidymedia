//! Windows 路径专项测试：中文目录、中文文件名、长根路径。
//!
//! 整文件 `#![cfg(windows)]` gated — Linux 不参与编译（`camino::Utf8Path` 在 Linux
//! 不把 `\` 当分隔符，相关 invariants 仅在 Windows 上成立）。
//! 对应验收手册 §异常应对表 「中文路径乱码」「路径超过 260 字符」两行的契约。

#![cfg(windows)]

use tempfile::tempdir;
use tidymedia::{CommandResult, Commands, tidy_with};

use super::{DATA_DIR, FakeBackendFactory, local};

fn copy_fixture(dst: &std::path::Path) {
    std::fs::copy(format!("{DATA_DIR}/sample-with-offset.jpg"), dst).expect("copy fixture");
}

// 中文目录：src 在「照片」子目录，归档到 out/2024/05/。
#[test]
fn copy_with_chinese_src_directory_preserves_chars() {
    let root = tempdir().unwrap();
    let src_dir = root.path().join("照片");
    std::fs::create_dir(&src_dir).unwrap();
    copy_fixture(&src_dir.join("sample-with-offset.jpg"));

    let out = tempdir().unwrap();

    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![local(src_dir.to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy from chinese-named dir");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };

    assert_eq!(report.failed, 0, "report: {report:?}");
    assert!(
        out.path().join("2024").join("05").is_dir(),
        "expected out/2024/05 to exist"
    );
}

// 中文文件名：归档后文件名保留中文。
#[test]
fn copy_preserves_chinese_filename() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("春节合影.jpg");
    copy_fixture(&src_file);

    let out = tempdir().unwrap();

    let factory = FakeBackendFactory::new();
    tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy with chinese filename");

    let archived = out.path().join("2024").join("05").join("春节合影.jpg");
    assert!(
        archived.exists(),
        "expected chinese filename preserved at {archived:?}"
    );
}

// 长根路径：out 根 + archive_template 嵌套接近 / 超过 MAX_PATH (260)。
// 不假设具体失败/成功（取决于 NTFS 长路径开关），但 errors 不能静默丢数据 —
// 失败必须出现在 report.errors 或 failed 计数。
#[test]
fn copy_into_long_root_path_reports_failures_if_any() {
    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("sample-with-offset.jpg");
    copy_fixture(&src_file);

    let out_root = tempdir().unwrap();
    // 在 out_root 下构造一个深度足以让最终路径 > 260 字符的子目录链。
    // 每段 32 字符，10 段 ≈ 320 字符再 +/2024/05/sample-with-offset.jpg。
    let mut deep = out_root.path().to_path_buf();
    for _ in 0..10 {
        deep = deep.join("a".repeat(32));
    }

    let factory = FakeBackendFactory::new();
    let result = tidy_with(
        &factory,
        Commands::Copy {
            dry_run: false,
            include_non_media: false,
            sources: vec![local(src_dir.path().to_str().unwrap())],
            output: local(deep.to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    );

    // 启用了长路径支持 → Ok + 文件落盘；未启用 → Err 或 report.failed > 0。
    // 两种结果都可接受，但不能静默成功（copied >= 1 但文件实际未写）。
    if let Ok(CommandResult::Copy(report)) = result {
        if report.copied > 0 {
            let archived = deep.join("2024").join("05").join("sample-with-offset.jpg");
            assert!(
                archived.exists(),
                "report claims copied but file missing at {archived:?}; report={report:?}"
            );
        } else {
            assert!(
                report.failed > 0,
                "must not silently report 0 copied AND 0 failed: {report:?}"
            );
        }
    }
}
