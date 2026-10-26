use std::io;

use camino::Utf8Path;

use super::*;
use crate::usecases::config::{CategoryDef, ClassifyConfig};

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
    let c = build_classifier(&cfg_with_categories(Vec::new())).unwrap();
    let r = c.classify(Utf8Path::new("/a.pdf"), "any").unwrap();
    assert_eq!(r.category, "");
    assert!(r.score.is_infinite() && r.score.is_sign_negative());
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
    assert_eq!(r.category, "");
    assert!(r.score.is_infinite() && r.score.is_sign_negative());
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
    assert_ne!(r.category, "");
}

#[test]
fn normalize_infinite_norm_unchanged() {
    let v = normalize(vec![f32::INFINITY, 0.0]);
    assert_eq!(v, vec![f32::INFINITY, 0.0]);
}

#[test]
fn best_match_finite_beats_trailing_nan() {
    let protos = vec![
        ("ok_cat".to_string(), vec![1.0]),
        ("nan_cat".to_string(), vec![f32::NAN]),
    ];
    let r = best_match(&[0.5], &protos);
    assert_eq!(r.category, "ok_cat");
}

#[test]
fn pad_truncates_long_token_sequence_to_seq() {
    let long: Vec<u32> = (0..300).collect();
    let out = pad_i64(&long);
    assert_eq!(out.len(), SEQ);
    assert_eq!(out[0], 0);
    assert_eq!(out[SEQ - 1], 255);
}

#[test]
fn pad_pads_short_sequence_with_zero_ids() {
    let out = pad_i64(&[7, 9]);
    assert_eq!(out.len(), SEQ);
    assert_eq!(&out[..2], &[7, 9]);
    assert!(out[2..].iter().all(|&x| x == 0));
}

#[test]
fn pad_preserves_exact_length_sequence() {
    let exact: Vec<u32> = (0..256).collect();
    let out = pad_i64(&exact);
    let expected: Vec<i64> = (0..256).collect();
    assert_eq!(out, expected);
}

struct FailOnTextEmbedder {
    ok: Vec<f32>,
    trigger: &'static str,
}

impl RawEmbedder for FailOnTextEmbedder {
    fn embed(&self, text: &str) -> io::Result<Vec<f32>> {
        if text.contains(self.trigger) {
            return Err(io::Error::other("stub inference failure"));
        }
        Ok(self.ok.clone())
    }
}

#[test]
fn classify_embed_failure_at_inference_propagates() {
    let c = TractEmbedClassifier::with_raw(
        cfg_with_categories(vec![cat("x", "stable description")]),
        Box::new(FailOnTextEmbedder {
            ok: vec![1.0],
            trigger: "boom",
        }),
    )
    .unwrap();
    let err = c
        .classify(Utf8Path::new("/a.pdf"), "boom text")
        .unwrap_err();
    assert!(
        err.to_string().contains("stub inference failure"),
        "got: {err}"
    );
}

#[test]
fn classify_unloaded_propagates_loader_error() {
    let c = build_classifier(&cfg_with_categories(vec![cat("x", "d")])).unwrap();
    let err = c.classify(Utf8Path::new("/a.pdf"), "any").unwrap_err();
    assert!(err.to_string().contains("load tokenizer"), "got: {err}");
}

static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[expect(clippy::unnecessary_wraps, reason = "与生产 loader 签名一致")]
fn load_counting(_: &ClassifyConfig) -> io::Result<Box<dyn RawEmbedder>> {
    LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(Box::new(StubEmbedder {
        table: Vec::new(),
        fallback: vec![1.0, 0.0],
    }))
}

#[test]
fn ensure_loaded_builds_prototypes_once() {
    LOADS.store(0, std::sync::atomic::Ordering::SeqCst);
    let c = TractEmbedClassifier {
        cfg: cfg_with_categories(vec![cat("x", "d")]),
        state: Mutex::new(None),
    };
    c.ensure_loaded_with(load_counting).unwrap();
    c.ensure_loaded_with(load_counting).unwrap();
    let state = c.state.lock();
    assert_eq!(state.as_ref().unwrap().prototypes.len(), 1);
    assert_eq!(LOADS.load(std::sync::atomic::Ordering::SeqCst), 1);
}
