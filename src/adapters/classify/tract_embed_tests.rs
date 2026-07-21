//! stub `RawEmbedder` 注入验证 normalize / cosine / argmax / 阈值语义 +
//! `build_classifier` 装配守卫。真实 tract 推理在 `tract_embed_real.rs`
//! （覆盖率 ignore-regex 排除，e2e 由真模型手动验证）。

use std::io;

use camino::Utf8Path;

use super::*;
use crate::usecases::config::{CategoryDef, ClassifyConfig};

/// 文本查表 stub：按 needle 子串返回预设向量；miss 返回 `fallback`。
struct StubEmbedder {
    table: Vec<(&'static str, Vec<f32>)>,
    fallback: Vec<f32>,
}

impl RawEmbedder for StubEmbedder {
    fn embed(&self, text: &str) -> io::Result<Vec<f32>> {
        for (needle, v) in &self.table {
            if text.contains(needle) {
                return Ok(v.clone());
            }
        }
        Ok(self.fallback.clone())
    }
}

/// 恒 Err stub：验证原型构建失败传播。
struct FailEmbedder;

impl RawEmbedder for FailEmbedder {
    fn embed(&self, _text: &str) -> io::Result<Vec<f32>> {
        Err(io::Error::other("stub embed failure"))
    }
}

fn cfg_with_categories(categories: Vec<CategoryDef>) -> ClassifyConfig {
    ClassifyConfig {
        embed_model_path: "unused-by-stub.onnx".into(),
        tokenizer_path: "unused-by-stub.json".into(),
        categories,
        ..ClassifyConfig::default()
    }
}

fn cat(name: &str, desc: &str) -> CategoryDef {
    CategoryDef {
        name: name.into(),
        description: desc.into(),
    }
}

#[test]
fn classify_picks_nearest_prototype() {
    let stub = StubEmbedder {
        table: vec![
            ("发票描述", vec![1.0, 0.0]),
            ("合同描述", vec![0.0, 1.0]),
            ("这是发票", vec![0.9, 0.1]),
        ],
        fallback: vec![0.0, 0.0],
    };
    let c = TractEmbedClassifier::with_raw(
        cfg_with_categories(vec![
            cat("invoice", "发票描述"),
            cat("contract", "合同描述"),
        ]),
        Box::new(stub),
    )
    .unwrap();
    let r = c.classify(Utf8Path::new("/a.pdf"), "这是发票").unwrap();
    assert_eq!(r.category, "invoice");
    assert!(r.score > 0.9, "got score {}", r.score);
}

#[test]
fn classify_empty_categories_short_circuits_without_model() {
    // categories 空 → 不触发 ensure_loaded（state 为 None 也不 panic），
    // 返回空类目 + -inf 让任何阈值裁决为 uncategorized。
    let c = build_classifier(&cfg_with_categories(Vec::new())).unwrap();
    let r = c.classify(Utf8Path::new("/a.pdf"), "any").unwrap();
    assert!(r.category.is_empty());
    assert!(r.score == f32::NEG_INFINITY);
}

#[test]
fn prototype_build_propagates_embed_error() {
    let err = TractEmbedClassifier::with_raw(
        cfg_with_categories(vec![cat("x", "desc")]),
        Box::new(FailEmbedder),
    )
    .unwrap_err();
    assert!(err.to_string().contains("stub embed failure"), "got: {err}");
}

#[test]
fn build_classifier_rejects_empty_model_path() {
    let cfg = ClassifyConfig {
        embed_model_path: String::new(),
        ..cfg_with_categories(vec![cat("x", "d")])
    };
    let err = build_classifier(&cfg).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn build_classifier_rejects_empty_tokenizer_path() {
    let cfg = ClassifyConfig {
        tokenizer_path: String::new(),
        ..cfg_with_categories(vec![cat("x", "d")])
    };
    let err = build_classifier(&cfg).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn debug_reports_loaded_state() {
    let c = TractEmbedClassifier::with_raw(
        cfg_with_categories(vec![cat("x", "d")]),
        Box::new(StubEmbedder {
            table: Vec::new(),
            fallback: vec![1.0],
        }),
    )
    .unwrap();
    let s = format!("{c:?}");
    assert!(s.contains("loaded: true"), "got: {s}");
}

// ============= normalize / best_match 纯函数 =============

#[test]
fn normalize_unit_length() {
    let v = normalize(vec![3.0, 4.0]);
    assert!((v[0] - 0.6).abs() < 1e-6);
    assert!((v[1] - 0.8).abs() < 1e-6);
}

#[test]
fn normalize_zero_vector_unchanged() {
    assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
}

#[test]
fn normalize_nan_norm_unchanged() {
    let v = normalize(vec![f32::NAN, 1.0]);
    assert!(v[0].is_nan());
}

#[test]
fn best_match_empty_prototypes_returns_neg_inf() {
    let r = best_match(&[1.0], &[]);
    assert!(r.category.is_empty());
    assert!(r.score == f32::NEG_INFINITY);
}

#[test]
fn best_match_nan_score_loses_to_finite() {
    let protos = vec![
        ("nan_cat".to_string(), vec![f32::NAN]),
        ("ok_cat".to_string(), vec![1.0]),
    ];
    let r = best_match(&[0.5], &protos);
    assert_eq!(r.category, "ok_cat");
}

#[test]
fn best_match_all_nan_still_returns_some_category() {
    let protos = vec![
        ("a".to_string(), vec![f32::NAN]),
        ("b".to_string(), vec![f32::NAN]),
    ];
    let r = best_match(&[1.0], &protos);
    assert!(r.score.is_nan());
    assert!(!r.category.is_empty());
}
