#[test]
fn mime_from_ext_numbers() {
    assert_eq!(
        super::mime_from_ext(Some("numbers")),
        Some("application/vnd.apple.numbers")
    );
}

#[test]
fn mime_from_ext_keynote() {
    assert_eq!(
        super::mime_from_ext(Some("key")),
        Some("application/vnd.apple.keynote")
    );
}

#[test]
fn mime_from_ext_odt() {
    assert!(
        super::mime_from_ext(Some("odt"))
            .unwrap()
            .ends_with("opendocument.text")
    );
}

#[test]
fn mime_from_ext_ods() {
    assert!(
        super::mime_from_ext(Some("ods"))
            .unwrap()
            .ends_with("opendocument.spreadsheet")
    );
}

#[test]
fn mime_from_ext_odp() {
    assert!(
        super::mime_from_ext(Some("odp"))
            .unwrap()
            .ends_with("opendocument.presentation")
    );
}

#[test]
fn mime_from_ext_odg() {
    assert!(
        super::mime_from_ext(Some("odg"))
            .unwrap()
            .ends_with("opendocument.graphics")
    );
}

#[test]
fn mime_from_ext_rtf() {
    assert_eq!(super::mime_from_ext(Some("rtf")), Some("application/rtf"));
}

#[test]
fn mime_from_ext_epub() {
    assert_eq!(
        super::mime_from_ext(Some("epub")),
        Some("application/epub+zip")
    );
}

#[test]
fn mime_from_ext_xmind() {
    assert_eq!(
        super::mime_from_ext(Some("xmind")),
        Some("application/vnd.xmind.workbook")
    );
}

#[test]
fn mime_from_ext_mm() {
    assert_eq!(
        super::mime_from_ext(Some("mm")),
        Some("application/x-freemind")
    );
}

#[test]
fn mime_from_ext_mindnode() {
    assert_eq!(
        super::mime_from_ext(Some("mindnode")),
        Some("application/x-mindnode")
    );
}

#[test]
fn mime_from_ext_itmz() {
    assert_eq!(
        super::mime_from_ext(Some("itmz")),
        Some("application/x-itmz")
    );
}

#[test]
fn mime_from_ext_mmap() {
    assert_eq!(
        super::mime_from_ext(Some("mmap")),
        Some("application/x-mindmanager")
    );
}

#[test]
fn mime_from_ext_txt() {
    assert_eq!(super::mime_from_ext(Some("txt")), Some("text/plain"));
}

#[test]
fn mime_from_ext_log() {
    assert_eq!(super::mime_from_ext(Some("log")), Some("text/plain"));
}

#[test]
fn mime_from_ext_md() {
    assert_eq!(super::mime_from_ext(Some("md")), Some("text/markdown"));
}

#[test]
fn mime_from_ext_markdown() {
    assert_eq!(
        super::mime_from_ext(Some("markdown")),
        Some("text/markdown")
    );
}

#[test]
fn mime_from_ext_rst() {
    assert_eq!(super::mime_from_ext(Some("rst")), Some("text/x-rst"));
}

#[test]
fn mime_from_ext_csv() {
    assert_eq!(super::mime_from_ext(Some("csv")), Some("text/csv"));
}

#[test]
fn mime_from_ext_tsv() {
    assert_eq!(
        super::mime_from_ext(Some("tsv")),
        Some("text/tab-separated-values")
    );
}

#[test]
fn mime_from_ext_unknown_returns_none() {
    assert!(super::mime_from_ext(Some("unknownext")).is_none());
}

#[test]
fn mime_from_ext_none_input_returns_none() {
    assert!(super::mime_from_ext(None).is_none());
}

// `is_office_mime` 覆盖每个 `||` arm 真 + 一次假兜底。`mime_from_ext` 全集已让大部分
// arm true 路径被命中——专测 false arm + 边界 arm。

#[test]
fn is_office_mime_pdf_true() {
    assert!(super::is_office_mime("application/pdf"));
}

#[test]
fn is_office_mime_ooxml_true() {
    assert!(super::is_office_mime(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    ));
}

#[test]
fn is_office_mime_doc_true() {
    assert!(super::is_office_mime("application/msword"));
}

#[test]
fn is_office_mime_ppt_true() {
    assert!(super::is_office_mime("application/vnd.ms-powerpoint"));
}

#[test]
fn is_office_mime_xls_true() {
    assert!(super::is_office_mime("application/vnd.ms-excel"));
}

#[test]
fn is_office_mime_pages_true() {
    assert!(super::is_office_mime("application/vnd.apple.pages"));
}

#[test]
fn is_office_mime_numbers_true() {
    assert!(super::is_office_mime("application/vnd.apple.numbers"));
}

#[test]
fn is_office_mime_keynote_true() {
    assert!(super::is_office_mime("application/vnd.apple.keynote"));
}

#[test]
fn is_office_mime_iwork_prefix_true() {
    assert!(super::is_office_mime("application/x-iwork-pages-sffpages"));
}

#[test]
fn is_office_mime_odf_prefix_true() {
    assert!(super::is_office_mime(
        "application/vnd.oasis.opendocument.text"
    ));
}

#[test]
fn is_office_mime_rtf_app_true() {
    assert!(super::is_office_mime("application/rtf"));
}

#[test]
fn is_office_mime_rtf_text_true() {
    assert!(super::is_office_mime("text/rtf"));
}

#[test]
fn is_office_mime_epub_true() {
    assert!(super::is_office_mime("application/epub+zip"));
}

#[test]
fn is_office_mime_xmind_true() {
    assert!(super::is_office_mime("application/vnd.xmind.workbook"));
}

#[test]
fn is_office_mime_xmind_alt_true() {
    assert!(super::is_office_mime("application/x-xmind"));
}

#[test]
fn is_office_mime_freemind_true() {
    assert!(super::is_office_mime("application/x-freemind"));
}

#[test]
fn is_office_mime_mindnode_true() {
    assert!(super::is_office_mime("application/x-mindnode"));
}

#[test]
fn is_office_mime_itmz_true() {
    assert!(super::is_office_mime("application/x-itmz"));
}

#[test]
fn is_office_mime_mindmanager_true() {
    assert!(super::is_office_mime("application/x-mindmanager"));
}
