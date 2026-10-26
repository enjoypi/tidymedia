use super::{strip_markup_into, truncate_at_boundary};

#[test]
fn strip_markup_into_folds_and_truncates() {
    let mut out = String::new();
    strip_markup_into(b"<p>hello <b>world</b></p>", &mut out, 64);
    assert_eq!(out, "hello world");
    truncate_at_boundary(&mut out, 4);
    assert_eq!(out, "hell");
}

#[test]
fn strip_markup_into_empty_result_skips_push() {
    let mut out = String::new();
    strip_markup_into(b"<a>   </a>", &mut out, 64);
    assert_eq!(out, "");
}
