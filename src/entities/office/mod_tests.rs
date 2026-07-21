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

// ============= extract_office_text 路由 + zip 容器入口行为 =============

use std::io::Write;

fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }
    cursor.into_inner()
}

fn route_text(buf: &[u8], mime: &str) -> String {
    let mut r = Cursor::new(buf.to_vec());
    extract_office_text(&mut r, mime, 4096)
}

#[test]
fn text_plain_extracts_raw_body() {
    assert_eq!(
        route_text(b"raw body text", MIME_TEXT_PLAIN),
        "raw body text"
    );
}

#[test]
fn docx_extracts_document_xml_body() {
    let z = make_zip(&[(
        "word/document.xml",
        "<w:document><w:body><w:p><w:r><w:t>发票 报销</w:t></w:r></w:p></w:body></w:document>"
            .as_bytes(),
    )]);
    assert_eq!(route_text(&z, MIME_DOCX), "发票 报销");
}

#[test]
fn xlsx_extracts_shared_strings() {
    let z = make_zip(&[(
        "xl/sharedStrings.xml",
        b"<sst><si><t>cell one</t></si><si><t>cell two</t></si></sst>",
    )]);
    assert_eq!(route_text(&z, MIME_XLSX), "cell one cell two");
}

#[test]
fn pptx_extracts_slides_in_order() {
    let z = make_zip(&[
        (
            "ppt/slides/slide2.xml",
            b"<p:sp><a:t>second</a:t></p:sp>".as_slice(),
        ),
        (
            "ppt/slides/slide1.xml",
            b"<p:sp><a:t>first</a:t></p:sp>".as_slice(),
        ),
    ]);
    assert_eq!(route_text(&z, MIME_PPTX), "first second");
}

#[test]
fn odt_extracts_content_xml() {
    let z = make_zip(&[(
        "content.xml",
        "<office:body><text:p>合同条款正文</text:p></office:body>".as_bytes(),
    )]);
    assert_eq!(route_text(&z, MIME_ODT), "合同条款正文");
}

#[test]
fn epub_extracts_xhtml_chapters() {
    let z = make_zip(&[
        ("mimetype", b"application/epub+zip".as_slice()),
        (
            "OEBPS/ch1.xhtml",
            b"<html><body><p>chapter body</p></body></html>",
        ),
    ]);
    assert_eq!(route_text(&z, MIME_EPUB), "chapter body");
}

#[test]
fn xmind_classic_extracts_content_xml() {
    let z = make_zip(&[(
        "content.xml",
        b"<xmap-content><topic><title>root topic</title></topic></xmap-content>".as_slice(),
    )]);
    assert_eq!(route_text(&z, MIME_XMIND), "root topic");
}

#[test]
fn xmind_zen_falls_back_to_content_json() {
    let z = make_zip(&[("content.json", br#"[{"title":"zen topic"}]"#.as_slice())]);
    assert_eq!(route_text(&z, MIME_XMIND), "zen topic");
}

#[test]
fn freemind_extracts_text_attrs() {
    assert_eq!(
        route_text(br#"<map><node TEXT="mind node"/></map>"#, MIME_FREEMIND),
        "mind node"
    );
}

#[test]
fn iwork_returns_empty_known_limitation() {
    assert_eq!(route_text(b"PK", MIME_PAGES), "");
}

#[test]
fn rtf_routes_to_strip_rtf() {
    assert_eq!(route_text(b"{\\rtf1 body}", MIME_RTF_APP), "body");
}

#[test]
fn pdf_routes_to_text_layer_scan() {
    let pdf = b"%PDF-1.4\n<< >>\nstream\nBT (pdf body) Tj ET\nendstream\n";
    assert_eq!(route_text(pdf, MIME_PDF), "pdf body");
}

#[test]
fn cfb_mime_with_non_cfb_bytes_returns_empty() {
    assert_eq!(route_text(b"not a compound file", MIME_DOC), "");
}

#[test]
fn unknown_mime_falls_back_to_raw_text() {
    assert_eq!(
        route_text(b"opaque", "application/x-unknown-future"),
        "opaque"
    );
}
