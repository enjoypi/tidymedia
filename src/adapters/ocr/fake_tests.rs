use super::*;

#[test]
fn fake_returns_default_on_miss() {
    let d = FakeTextDetector::new(false);
    assert!(!d.has_text(Utf8Path::new("/x.png"), b"").unwrap());

    let d = FakeTextDetector::new(true);
    assert!(d.has_text(Utf8Path::new("/x.png"), b"").unwrap());
}

#[test]
fn fake_returns_explicit_result() {
    let d = FakeTextDetector::new(false).with_result("/a.png", true);
    assert!(d.has_text(Utf8Path::new("/a.png"), b"").unwrap());
    assert!(!d.has_text(Utf8Path::new("/b.png"), b"").unwrap());
}

#[test]
fn fake_injected_error_takes_precedence_over_result() {
    let d = FakeTextDetector::new(true)
        .with_result("/err.png", true)
        .with_error("/err.png");
    let e = d.has_text(Utf8Path::new("/err.png"), b"").unwrap_err();
    assert!(e.to_string().contains("injected error"));
}

#[test]
fn fake_debug_redacts_internal_maps() {
    let d = FakeTextDetector::new(true)
        .with_result("/a", true)
        .with_error("/b");
    let s = format!("{d:?}");
    assert!(s.contains("FakeTextDetector"), "got: {s}");
    assert!(s.contains("results_count: 1"), "got: {s}");
    assert!(s.contains("errors_count: 1"), "got: {s}");
}
