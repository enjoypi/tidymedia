//! `make_classify_provider` / `classify_one` / `template_needs_category` 的
//! 分支断言：`FakeBackend` 喂字节 + `FakeDocumentClassifier` 注入规则。

use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::classify::fake::FakeDocumentClassifier;
use crate::entities::backend::Backend;

fn fake_with_file(path: &str, bytes: &[u8]) -> (Location, Arc<dyn Backend>) {
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(Utf8PathBuf::from(path));
    fake.add_file(loc.clone(), bytes.to_vec());
    (loc, fake)
}

#[test]
fn template_needs_category_true_when_placeholder_present() {
    assert!(template_needs_category("{category}/{year}"));
}

#[test]
fn template_needs_category_false_without_placeholder() {
    assert!(!template_needs_category("{year}/{month}"));
}

#[test]
fn provider_returns_category_when_rule_hits_above_threshold() {
    let (loc, backend) = fake_with_file("/in-mem/发票.txt", "增值税发票 抬头".as_bytes());
    let classifier =
        Arc::new(FakeDocumentClassifier::new("misc").with_rule("发票", "invoice", 0.9));
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(
        provider(&loc, &backend, "text/plain"),
        Some("invoice".to_string())
    );
}

#[test]
fn provider_returns_none_below_threshold() {
    // FakeDocumentClassifier miss → default_category + score 0.0 < 0.5 → None。
    let (loc, backend) = fake_with_file("/in-mem/notes.txt", b"unrelated body");
    let classifier = Arc::new(FakeDocumentClassifier::new("misc"));
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(provider(&loc, &backend, "text/plain"), None);
}

#[test]
fn provider_returns_none_when_no_text_extractable() {
    // iWork mime 提取恒空（IWA 不解析）→ no_text 分支。
    let (loc, backend) = fake_with_file("/in-mem/deck.key", b"PK\x03\x04whatever");
    let classifier = Arc::new(FakeDocumentClassifier::new("misc").with_rule("PK", "hit", 0.9));
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(
        provider(&loc, &backend, "application/vnd.apple.keynote"),
        None
    );
}

#[test]
fn provider_returns_none_on_classifier_error() {
    let (loc, backend) = fake_with_file("/in-mem/bad.txt", b"content here");
    let classifier = Arc::new(
        FakeDocumentClassifier::new("misc")
            .with_rule("content", "hit", 0.9)
            .with_error("/in-mem/bad.txt"),
    );
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(provider(&loc, &backend, "text/plain"), None);
}

#[test]
fn provider_returns_none_on_open_read_error() {
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(Utf8PathBuf::from("/in-mem/missing.txt"));
    let backend: Arc<dyn Backend> = fake;
    let classifier = Arc::new(FakeDocumentClassifier::new("misc").with_rule("x", "hit", 0.9));
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(provider(&loc, &backend, "text/plain"), None);
}

#[test]
fn provider_score_at_threshold_passes() {
    // score == score_min 不低于阈值 → 通过（`<` 语义）。
    let (loc, backend) = fake_with_file("/in-mem/edge.txt", b"edge content");
    let classifier = Arc::new(FakeDocumentClassifier::new("misc").with_rule("edge", "cat", 0.5));
    let provider = make_classify_provider(classifier, 4096, 0.5);
    assert_eq!(provider(&loc, &backend, "text/plain"), Some("cat".into()));
}
