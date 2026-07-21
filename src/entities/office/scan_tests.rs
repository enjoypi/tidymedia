use super::*;

#[test]
fn strip_markup_extracts_text_between_tags() {
    let mut out = String::new();
    strip_markup_into(b"<w:t>hello</w:t><w:t>world</w:t>", &mut out, 64);
    assert_eq!(out, "hello world");
}

#[test]
fn strip_markup_folds_whitespace_runs() {
    let mut out = String::new();
    strip_markup_into(b"<p>a  \n\t b</p>", &mut out, 64);
    assert_eq!(out, "a b");
}

#[test]
fn strip_markup_respects_budget() {
    let mut out = String::new();
    strip_markup_into(b"<t>abcdefgh</t>", &mut out, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn strip_markup_zero_budget_appends_nothing() {
    let mut out = String::from("full");
    strip_markup_into(b"<t>more</t>", &mut out, 4);
    assert_eq!(out, "full");
}

#[test]
fn strip_markup_appends_across_calls() {
    let mut out = String::new();
    strip_markup_into(b"<t>one</t>", &mut out, 64);
    strip_markup_into(b"<t>two</t>", &mut out, 64);
    assert_eq!(out, "one two");
}

#[test]
fn strip_markup_handles_utf8_cjk_text() {
    let mut out = String::new();
    strip_markup_into("<text:p>增值税发票</text:p>".as_bytes(), &mut out, 64);
    assert_eq!(out, "增值税发票");
}

#[test]
fn strip_markup_tag_with_attributes_excluded() {
    let mut out = String::new();
    strip_markup_into(b"<w:t xml:space=\"preserve\">kept</w:t>", &mut out, 64);
    assert_eq!(out, "kept");
}

#[test]
fn strip_markup_unclosed_tag_drops_tail() {
    let mut out = String::new();
    strip_markup_into(b"text<unclosed attr", &mut out, 64);
    assert_eq!(out, "text");
}

#[test]
fn truncate_at_boundary_noop_when_short() {
    let mut s = String::from("ok");
    truncate_at_boundary(&mut s, 10);
    assert_eq!(s, "ok");
}

#[test]
fn truncate_at_boundary_backs_off_multibyte_split() {
    // "发" 是 3 字节；max=4 落在第二个字符中间 → 回退到 3。
    let mut s = String::from("发票");
    truncate_at_boundary(&mut s, 4);
    assert_eq!(s, "发");
}

#[test]
fn truncate_at_boundary_exact_boundary_kept() {
    let mut s = String::from("发票");
    truncate_at_boundary(&mut s, 3);
    assert_eq!(s, "发");
}

#[test]
fn truncate_at_boundary_zero_empties_string() {
    let mut s = String::from("abc");
    truncate_at_boundary(&mut s, 0);
    assert_eq!(s, "");
}
