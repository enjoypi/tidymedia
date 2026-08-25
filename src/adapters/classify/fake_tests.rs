use super::*;

#[test]
fn fake_matches_first_rule_by_substring() {
    let c = FakeDocumentClassifier::new("misc").with_rule("发票", "invoice", 0.9);
    let r = c
        .classify(Utf8Path::new("/a.pdf"), "增值税发票抬头")
        .unwrap();
    assert_eq!(r.category, "invoice");
}

#[test]
fn fake_rule_score_passthrough() {
    let c = FakeDocumentClassifier::new("misc").with_rule("x", "cat", 0.7);
    let r = c.classify(Utf8Path::new("/a"), "x").unwrap();
    assert!((r.score - 0.7).abs() < f32::EPSILON);
}

#[test]
fn fake_miss_returns_default_with_low_score() {
    let c = FakeDocumentClassifier::new("misc").with_rule("发票", "invoice", 0.9);
    let r = c.classify(Utf8Path::new("/a.txt"), "无关内容").unwrap();
    assert_eq!(r.category, "misc");
    assert!(r.score.abs() < f32::EPSILON);
}

#[test]
fn fake_injected_error_wins_over_rules() {
    let c = FakeDocumentClassifier::new("misc")
        .with_rule("x", "cat", 0.9)
        .with_error("/bad.doc");
    let err = c.classify(Utf8Path::new("/bad.doc"), "x").unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn fake_debug_redacts_internals() {
    let c = FakeDocumentClassifier::new("misc").with_rule("a", "b", 0.5);
    let s = format!("{c:?}");
    assert!(s.contains("rules_count"), "got: {s}");
}
