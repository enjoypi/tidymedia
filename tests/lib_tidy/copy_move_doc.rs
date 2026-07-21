//! `copy-doc` / `move-doc` 端到端：文档过滤、内容分类 `{category}` 桶、
//! move 删源、dry-run、`run_cli` 字符串形式、partial-failure 文案。

use tempfile::tempdir;
use tidymedia::{
    CommandResult, Commands, DefaultBackendFactory, DefaultDetectorFactory, DetectorFactory,
    DocumentClassifier, FakeDocumentClassifier, run_cli, tidy, tidy_with,
};

use super::{DATA_DIR, local};

/// 只提供 `DocumentClassifier` 的测试 factory：copy-doc/move-doc 不 build 其它
/// detector，5 个 face/ocr 方法恒 Err 兜底（被调用即测试错误）。
struct ClassifierOnlyFactory {
    rules: Vec<(&'static str, &'static str, f32)>,
}

impl DetectorFactory for ClassifierOnlyFactory {
    fn build_face_detector(&self) -> std::io::Result<Box<dyn tidymedia::FaceDetector>> {
        Err(std::io::Error::other("not used by copy-doc"))
    }

    fn build_face_embedder(&self) -> std::io::Result<Box<dyn tidymedia::FaceEmbedder>> {
        Err(std::io::Error::other("not used by copy-doc"))
    }

    fn build_face_mesh(&self) -> std::io::Result<Box<dyn tidymedia::FaceMeshDetector>> {
        Err(std::io::Error::other("not used by copy-doc"))
    }

    fn build_eye_state_classifier(
        &self,
    ) -> std::io::Result<Box<dyn tidymedia::EyeStateClassifier>> {
        Err(std::io::Error::other("not used by copy-doc"))
    }

    fn build_text_detector(&self) -> std::io::Result<Box<dyn tidymedia::TextDetector>> {
        Err(std::io::Error::other("not used by copy-doc"))
    }

    fn build_document_classifier(&self) -> std::io::Result<Box<dyn DocumentClassifier>> {
        let mut fake = FakeDocumentClassifier::new("misc");
        for (needle, category, score) in &self.rules {
            fake = fake.with_rule(*needle, *category, *score);
        }
        Ok(Box::new(fake))
    }
}

fn seed(dir: &std::path::Path, name: &str) {
    std::fs::copy(format!("{DATA_DIR}/{name}"), dir.join(name))
        .unwrap_or_else(|e| panic!("seed {name}: {e}"));
}

fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

// 混合源：pdf（文档）+ jpg（媒体）+ bin（未知）。copy-doc 仅归档 pdf。
#[test]
fn copy_doc_archives_office_skips_media_and_unknown() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    seed(src.path(), "sample-with-exif.jpg");
    std::fs::write(src.path().join("blob.bin"), vec![0xDE_u8; 4096]).unwrap();
    let out = tempdir().unwrap();

    let factory = DefaultBackendFactory;
    let detectors = ClassifierOnlyFactory { rules: Vec::new() };
    let result = tidy_with(
        &factory,
        &detectors,
        Commands::CopyDoc {
            dry_run: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy-doc should succeed");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    assert_eq!(report.copied, 1, "only the pdf may be archived: {report:?}");
    assert!(report.doc_only, "report must carry doc_only marker");
    let archived = walk(out.path());
    assert_eq!(archived.len(), 1, "got: {archived:?}");
    assert!(
        archived[0]
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf")),
        "got: {archived:?}"
    );
}

// FakeDocumentClassifier 规则命中：docx 正文含「发票」→ invoice/2017/02 桶。
#[test]
fn copy_doc_renders_category_bucket_from_classifier() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-docx-dated.docx");
    let out = tempdir().unwrap();

    let factory = DefaultBackendFactory;
    let detectors = ClassifierOnlyFactory {
        rules: vec![("发票", "invoice", 0.9)],
    };
    tidy_with(
        &factory,
        &detectors,
        Commands::CopyDoc {
            dry_run: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy-doc should succeed");
    // dcterms:created=2017-02-14 + 默认模板 {category}/{year}/{month}
    let bucket = out.path().join("invoice").join("2017").join("02");
    assert!(
        bucket.exists(),
        "docx must land in invoice/2017/02; out tree: {:?}",
        walk(out.path())
    );
}

// Fake miss → score 0.0 < score_min 0.5 → uncategorized 桶（照常按时间归档）。
#[test]
fn copy_doc_low_score_falls_back_to_uncategorized() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();

    let factory = DefaultBackendFactory;
    let detectors = ClassifierOnlyFactory { rules: Vec::new() };
    tidy_with(
        &factory,
        &detectors,
        Commands::CopyDoc {
            dry_run: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy-doc should succeed");
    let bucket = out.path().join("uncategorized").join("2017").join("02");
    assert!(
        bucket.exists(),
        "unclassified pdf must land in uncategorized/2017/02; out tree: {:?}",
        walk(out.path())
    );
}

// move-doc：文档删源归档；媒体文件原地不动（filter skip 在 remove 之前）。
#[test]
fn move_doc_removes_source_doc_leaves_media_untouched() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    seed(src.path(), "sample-with-exif.jpg");
    let out = tempdir().unwrap();

    let factory = DefaultBackendFactory;
    let detectors = ClassifierOnlyFactory { rules: Vec::new() };
    tidy_with(
        &factory,
        &detectors,
        Commands::MoveDoc {
            dry_run: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("move-doc should succeed");
    assert!(
        !src.path().join("sample-pdf-dated.pdf").exists(),
        "moved document must be removed from source"
    );
    assert!(
        src.path().join("sample-with-exif.jpg").exists(),
        "media file must stay untouched in source"
    );
    assert_eq!(walk(out.path()).len(), 1);
}

#[test]
fn copy_doc_dry_run_does_not_write() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();

    let factory = DefaultBackendFactory;
    let detectors = ClassifierOnlyFactory { rules: Vec::new() };
    let result = tidy_with(
        &factory,
        &detectors,
        Commands::CopyDoc {
            dry_run: true,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect("copy-doc dry-run should succeed");
    let CommandResult::Copy(report) = result else {
        panic!("expected Copy report");
    };
    assert_eq!(report.copied, 1, "dry-run still counts would-copy");
    assert!(
        walk(out.path()).is_empty(),
        "dry-run must not write any file"
    );
}

// CLAUDE.md 强制：新增子命令 MUST 有 `run_cli([...])` 字符串形式 e2e。
// 显式模板避开 {category}（不构造分类器 → 不依赖模型文件）。
#[test]
fn run_cli_copy_doc_string_form() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "copy-doc",
        "--archive-template",
        "{year}/{month}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("run_cli copy-doc should succeed");
    assert!(out.path().join("2017").join("02").exists());
}

#[test]
fn run_cli_move_doc_string_form() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();
    run_cli([
        "tidymedia",
        "move-doc",
        "--archive-template",
        "{year}/{month}",
        "--output",
        out.path().to_str().unwrap(),
        src.path().to_str().unwrap(),
    ])
    .expect("run_cli move-doc should succeed");
    assert!(!src.path().join("sample-pdf-dated.pdf").exists());
    assert!(out.path().join("2017").join("02").exists());
}

// tidy() partial-failure 文案按 (remove, doc_only) 分流：copy-doc / move-doc。
// unique-name 耗尽套路（塞满原名 + _1..=_10）；显式模板避开分类器。
fn exhaust_bucket(out: &std::path::Path) {
    let sub = out.join("2017").join("02");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("sample-pdf-dated.pdf"), b"x").unwrap();
    for i in 1..=10 {
        std::fs::write(sub.join(format!("sample-pdf-dated_{i}.pdf")), b"x").unwrap();
    }
}

#[test]
fn tidy_returns_err_when_copy_doc_partial_failure() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();
    exhaust_bucket(out.path());

    let err = tidy(Commands::CopyDoc {
        dry_run: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year}/{month}".into()),
        report: None,
    })
    .expect_err("tidy must surface copy-doc partial failure");
    let msg = err.to_string();
    assert!(
        msg.contains("copy-doc partial failure"),
        "Err message must label sub-command as copy-doc: {msg}"
    );
}

#[test]
fn tidy_returns_err_when_move_doc_partial_failure() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();
    exhaust_bucket(out.path());

    let err = tidy(Commands::MoveDoc {
        dry_run: false,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: Some("{year}/{month}".into()),
        report: None,
    })
    .expect_err("tidy must surface move-doc partial failure");
    let msg = err.to_string();
    assert!(
        msg.contains("move-doc partial failure"),
        "Err message must label sub-command as move-doc: {msg}"
    );
}

// DefaultDetectorFactory 真实装配 + classify 模型路径为空：默认 doc 模板消费
// {category} → dispatch 构造分类器阶段 build_document_classifier 返 InvalidInput
// 快速失败（模板要 category 但模型未配，用户须感知配置缺失）。
// 覆盖 frameworks/detector.rs::build_document_classifier + dispatch `?` Err 传播。
#[test]
fn copy_doc_errs_when_classifier_model_unconfigured() {
    let cfg_dir = tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        "backend:\n  classify:\n    embed_model_path: \"\"\n    tokenizer_path: \"\"\n",
    )
    .unwrap();
    // SAFETY: nextest 每测试独立进程，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    tidymedia::install_config_loader();

    let src = tempdir().unwrap();
    seed(src.path(), "sample-pdf-dated.pdf");
    let out = tempdir().unwrap();
    let err = tidy_with(
        &DefaultBackendFactory,
        &DefaultDetectorFactory,
        Commands::CopyDoc {
            dry_run: true,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: None,
            report: None,
        },
    )
    .expect_err("unconfigured classifier model must fail fast");
    assert!(
        err.to_string().contains("classify"),
        "Err must point at classify config: {err}"
    );
}

// DefaultDetectorFactory 路径（真实装配）：build_document_classifier 懒加载，
// 模板无 {category} 时不构造——两工厂对照保证 dispatch 分支双向覆盖。
#[test]
fn copy_doc_with_default_factory_and_plain_template() {
    let src = tempdir().unwrap();
    seed(src.path(), "sample-docx-dated.docx");
    let out = tempdir().unwrap();
    tidy_with(
        &DefaultBackendFactory,
        &DefaultDetectorFactory,
        Commands::CopyDoc {
            dry_run: false,
            sources: vec![local(src.path().to_str().unwrap())],
            output: local(out.path().to_str().unwrap()),
            archive_template: Some("{year}/{month}".into()),
            report: None,
        },
    )
    .expect("copy-doc with plain template should succeed without classifier");
    assert!(out.path().join("2017").join("02").exists());
}
