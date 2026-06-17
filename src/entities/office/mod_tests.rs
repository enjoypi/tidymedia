//! `populate_office_dates` 路由 + `is_*_mime` helper 的分支覆盖（commit 1 装配阶段）。
//! 子模块 stub 全返 `(0, 0)`，所以本节断言全为 `(0, 0)`；后续 commit 在各子模块自测里
//! 覆盖具体解析逻辑，路由层只验「该 mime 进对哪条 if-arm」。

use std::io::Cursor;

use super::*;

fn route(buf: &[u8], mime: &str) -> (u64, u64) {
    let mut r = Cursor::new(buf.to_vec());
    populate_office_dates(&mut r, mime)
}

#[test]
fn pdf_mime_routes_to_pdf_stub() {
    assert_eq!(route(b"%PDF", MIME_PDF), (0, 0));
}

#[test]
fn docx_mime_routes_to_ooxml_stub() {
    assert_eq!(route(b"PK", MIME_DOCX), (0, 0));
}

#[test]
fn pptx_mime_routes_to_ooxml_stub() {
    assert_eq!(route(b"PK", MIME_PPTX), (0, 0));
}

#[test]
fn xlsx_mime_routes_to_ooxml_stub() {
    assert_eq!(route(b"PK", MIME_XLSX), (0, 0));
}

#[test]
fn doc_mime_routes_to_cfb_stub() {
    assert_eq!(route(b"\xD0\xCF", MIME_DOC), (0, 0));
}

#[test]
fn ppt_mime_routes_to_cfb_stub() {
    assert_eq!(route(b"\xD0\xCF", MIME_PPT), (0, 0));
}

#[test]
fn xls_mime_routes_to_cfb_stub() {
    assert_eq!(route(b"\xD0\xCF", MIME_XLS), (0, 0));
}

#[test]
fn pages_mime_routes_to_iwork_stub() {
    assert_eq!(route(b"PK", MIME_PAGES), (0, 0));
}

#[test]
fn numbers_mime_routes_to_iwork_stub() {
    assert_eq!(route(b"PK", MIME_NUMBERS), (0, 0));
}

#[test]
fn keynote_mime_routes_to_iwork_stub() {
    assert_eq!(route(b"PK", MIME_KEYNOTE), (0, 0));
}

#[test]
fn iwork_x_prefix_routes_to_iwork_stub() {
    assert_eq!(route(b"PK", "application/x-iwork-pages-sffpages"), (0, 0));
}

#[test]
fn odt_mime_routes_to_odf_stub() {
    assert_eq!(route(b"PK", MIME_ODT), (0, 0));
}

#[test]
fn rtf_app_mime_routes_to_rtf_stub() {
    assert_eq!(route(b"{\\rtf", MIME_RTF_APP), (0, 0));
}

#[test]
fn rtf_text_mime_routes_to_rtf_stub() {
    assert_eq!(route(b"{\\rtf", MIME_RTF_TEXT), (0, 0));
}

#[test]
fn epub_mime_routes_to_epub_stub() {
    assert_eq!(route(b"PK", MIME_EPUB), (0, 0));
}

#[test]
fn xmind_mime_routes_to_mindmap_zip_stub() {
    assert_eq!(route(b"PK", MIME_XMIND), (0, 0));
}

#[test]
fn xmind_alt_mime_routes_to_mindmap_zip_stub() {
    assert_eq!(route(b"PK", MIME_XMIND_ALT), (0, 0));
}

#[test]
fn mindnode_mime_routes_to_mindmap_zip_stub() {
    assert_eq!(route(b"PK", MIME_MINDNODE), (0, 0));
}

#[test]
fn itmz_mime_routes_to_mindmap_zip_stub() {
    assert_eq!(route(b"PK", MIME_ITMZ), (0, 0));
}

#[test]
fn mindmanager_mime_routes_to_mindmap_zip_stub() {
    assert_eq!(route(b"PK", MIME_MINDMANAGER), (0, 0));
}

#[test]
fn freemind_mime_routes_to_mindmap_mm_stub() {
    assert_eq!(route(b"<map", MIME_FREEMIND), (0, 0));
}

#[test]
fn text_plain_routes_to_text_stub() {
    assert_eq!(route(b"hello", MIME_TEXT_PLAIN), (0, 0));
}

#[test]
fn unrecognized_mime_routes_to_text_stub() {
    assert_eq!(route(b"\x00", "application/x-unknown-future"), (0, 0));
}

// `is_*_mime` helper 内部 `||` 多 arm 分支：populate_office_dates 上层调用已覆盖 true
// arm，专测 false arm 让 helper sub-branch 全到 100%。
#[test]
fn is_ooxml_mime_false_for_non_office() {
    assert!(!is_ooxml_mime("application/octet-stream"));
}

#[test]
fn is_cfb_mime_false_for_non_office() {
    assert!(!is_cfb_mime("application/octet-stream"));
}

#[test]
fn is_iwork_mime_false_for_non_office() {
    assert!(!is_iwork_mime("application/octet-stream"));
}

#[test]
fn is_mindmap_zip_mime_false_for_non_office() {
    assert!(!is_mindmap_zip_mime("application/octet-stream"));
}
