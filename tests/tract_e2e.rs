use std::fs;
use std::path::PathBuf;

use camino::Utf8Path;

use tidymedia::{CategoryDef, ClassifyConfig, OcrConfig, build_classifier, build_detector};

fn models() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")
}

fn photo_bytes() -> Vec<u8> {
    let p = format!(
        "{}/tests/data/sample-with-exif.jpg",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read(p).expect("sample photo fixture exists")
}

#[test]
fn embed_real_model_classifies() {
    let cfg = ClassifyConfig {
        embed_model_path: models()
            .join("bge_small_zh_v15_int8.onnx")
            .display()
            .to_string(),
        tokenizer_path: models()
            .join("bge_small_zh_v15_tokenizer.json")
            .display()
            .to_string(),
        categories: vec![CategoryDef {
            name: "趣味".to_string(),
            description: "日常记录与趣味内容".to_string(),
        }],
        score_min: 0.0,
        max_text_bytes: 4096,
    };
    let det = build_classifier(&cfg).unwrap();
    let c = det
        .classify(Utf8Path::new("/t.md"), "周末去公园散步拍的照片")
        .unwrap();
    assert!(c.category.contains("趣味") || c.score.is_finite());
}

#[test]
fn dbnet_missing_model_propagates_load_error() {
    let cfg = OcrConfig {
        det_model_path: models().join("missing-det.onnx").display().to_string(),
        ..OcrConfig::default()
    };
    let det = build_detector(&cfg).unwrap();
    let err = det
        .has_text(Utf8Path::new("/x.jpg"), &photo_bytes())
        .unwrap_err();
    assert!(err.to_string().contains("load"), "unexpected: {err}");
}

#[test]
fn embed_missing_paths_propagate_load_error() {
    let mk = |model: &str, tok: &str| ClassifyConfig {
        embed_model_path: models().join(model).display().to_string(),
        tokenizer_path: models().join(tok).display().to_string(),
        categories: vec![CategoryDef {
            name: "a".to_string(),
            description: "b".to_string(),
        }],
        score_min: 0.0,
        max_text_bytes: 64,
    };
    let det = build_classifier(&mk("bge_small_zh_v15_int8.onnx", "no-tokenizer.json")).unwrap();
    let err = det.classify(Utf8Path::new("/t"), "你好").unwrap_err();
    assert!(err.to_string().contains("load"), "tokenizer: {err}");
    let det = build_classifier(&mk("no-model.onnx", "bge_small_zh_v15_tokenizer.json")).unwrap();
    let err = det.classify(Utf8Path::new("/t"), "你好").unwrap_err();
    assert!(err.to_string().contains("load"), "model: {err}");
}

#[test]
fn embed_empty_categories_returns_empty_classification() {
    let cfg = ClassifyConfig {
        embed_model_path: models()
            .join("bge_small_zh_v15_int8.onnx")
            .display()
            .to_string(),
        tokenizer_path: models()
            .join("bge_small_zh_v15_tokenizer.json")
            .display()
            .to_string(),
        categories: vec![],
        score_min: 0.0,
        max_text_bytes: 64,
    };
    let det = build_classifier(&cfg).unwrap();
    let c = det.classify(Utf8Path::new("/t"), "任意文本").unwrap();
    assert!(c.category.is_empty() && c.score.is_infinite());
}

#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;
