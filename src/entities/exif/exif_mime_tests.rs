use super::Exif;
use super::tests_common::utc;

// 老 QuickTime `pnot` preview atom 起头的 MOV 文件：infer crate 只认 `ftyp`，
// 必须靠 fallback 兜底返回 `video/quicktime`，否则 `is_media` 误判致整文件被 ignore。
#[test]
fn quicktime_legacy_mime_detects_pnot_atom() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"pnot");
    assert_eq!(super::quicktime_legacy_mime(&buf), Some("video/quicktime"));
}

// mdat-first MOV 变体（无任何头 atom、moov 在文件末尾的早期 QuickTime）：
// `infer` 0.19 不识别 (无 ftyp)；旧实现仅查 pnot 也漏识 → MIME 为空 →
// from_reader 不调 populate_video_dates → fork 后的 nom-exif 永远拿不到执行机会
// → is_media() false，整段视频被 ignore（CLAUDE.md「项目 Gotcha」mdat-first 条）。
#[test]
fn quicktime_legacy_mime_detects_mdat_atom() {
    let mut buf = vec![0u8, 0x10, 0, 0]; // mdat 大 box size
    buf.extend_from_slice(b"mdat");
    buf.extend_from_slice(&[0u8; 32]); // 后续 body 字节（数 MB-级，此处仅占位）
    assert_eq!(super::quicktime_legacy_mime(&buf), Some("video/quicktime"));
}

#[test]
fn quicktime_legacy_mime_unknown_tag_returns_none() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"XXXX");
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}

#[test]
fn quicktime_legacy_mime_too_short_returns_none() {
    let buf = [0u8; 7];
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}

// BDAV M2TS（AVCHD .mts/.m2ts）：4-byte TP_extra_header + 188-byte TS packet。
// `infer` 0.19 不识别；fallback 要求 offset 4 + 196 连续两个 0x47 sync byte。
#[test]
fn m2ts_legacy_mime_detects_bdav_sync_pair() {
    let mut buf = vec![0u8; 256];
    buf[4] = 0x47;
    buf[196] = 0x47;
    assert_eq!(super::m2ts_legacy_mime(&buf), Some("video/m2ts"));
}

// 单 sync byte 不够 —— 任意二进制都可能在某 offset 命中 0x47。
#[test]
fn m2ts_legacy_mime_single_sync_returns_none() {
    let mut buf = vec![0u8; 256];
    buf[4] = 0x47;
    assert!(super::m2ts_legacy_mime(&buf).is_none());
}

#[test]
fn m2ts_legacy_mime_too_short_returns_none() {
    let buf = [0u8; 100];
    assert!(super::m2ts_legacy_mime(&buf).is_none());
}

// End-to-end：FakeBackend 喂 BDAV pattern bytes → Exif::open 走 m2ts fallback，
// 让 is_media() 通过门槛，整段 AVCHD 视频不被 ignore（之前 28 个 .MTS 文件残留场景）。
#[test]
fn open_uses_m2ts_legacy_fallback_for_bdav_pattern() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let mut bytes = vec![0u8; 256];
    bytes[4] = 0x47;
    bytes[196] = 0x47;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/clip.mts"));
    fake.add_file(loc.clone(), bytes);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "video/m2ts");
    assert!(exif.is_media());
}

// 3GPP 手机视频（常伪装 `.mp4` 扩展名）：标准 BMFF `ftyp` 但 brand 是 `3gp4`/`3gp5`；
// `infer` 0.19 的 MP4 matcher 不认 `3gp*` brand，不识别会让整段 3GP 被 ignore。
#[test]
fn bmff_3gpp_mime_detects_3gp_brand() {
    let mut buf = vec![0u8, 0, 0, 0x1c];
    buf.extend_from_slice(b"ftyp3gp5");
    assert_eq!(super::bmff_3gpp_mime(&buf), Some("video/3gpp"));
}

#[test]
fn bmff_3gpp_mime_other_brand_returns_none() {
    let mut buf = vec![0u8, 0, 0, 0x1c];
    buf.extend_from_slice(b"ftypisom");
    assert!(super::bmff_3gpp_mime(&buf).is_none());
}

#[test]
fn bmff_3gpp_mime_too_short_returns_none() {
    let buf = [0u8; 10];
    assert!(super::bmff_3gpp_mime(&buf).is_none());
}

// End-to-end：FakeBackend 喂 `ftyp3gp5` 头 → Exif::open 走 3gpp fallback，
// 让 is_media() 通过门槛（之前 7 个「录像NNNN.mp4」3GP 文件残留场景）。
#[test]
fn open_uses_3gpp_fallback_for_3gp_brand() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let mut bytes = vec![0u8, 0, 0, 0x1c];
    bytes.extend_from_slice(b"ftyp3gp5");
    bytes.resize(256, 0);

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/clip.mp4"));
    fake.add_file(loc.clone(), bytes);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "video/3gpp");
    assert!(exif.is_media());
}

// `mime_from_ext` 覆盖每个扩展名映射 arm（28 个 case + None + 大小写不敏感）。

#[test]
fn mime_from_ext_pdf() {
    assert_eq!(super::mime_from_ext(Some("pdf")), Some("application/pdf"));
}

#[test]
fn mime_from_ext_uppercase_pdf() {
    assert_eq!(super::mime_from_ext(Some("PDF")), Some("application/pdf"));
}

#[test]
fn mime_from_ext_docx() {
    assert!(
        super::mime_from_ext(Some("docx"))
            .unwrap()
            .ends_with("wordprocessingml.document")
    );
}

#[test]
fn mime_from_ext_pptx() {
    assert!(
        super::mime_from_ext(Some("pptx"))
            .unwrap()
            .ends_with("presentationml.presentation")
    );
}

#[test]
fn mime_from_ext_xlsx() {
    assert!(
        super::mime_from_ext(Some("xlsx"))
            .unwrap()
            .ends_with("spreadsheetml.sheet")
    );
}

#[test]
fn mime_from_ext_doc() {
    assert_eq!(
        super::mime_from_ext(Some("doc")),
        Some("application/msword")
    );
}

#[test]
fn mime_from_ext_ppt() {
    assert_eq!(
        super::mime_from_ext(Some("ppt")),
        Some("application/vnd.ms-powerpoint")
    );
}

#[test]
fn mime_from_ext_xls() {
    assert_eq!(
        super::mime_from_ext(Some("xls")),
        Some("application/vnd.ms-excel")
    );
}

#[test]
fn mime_from_ext_pages() {
    assert_eq!(
        super::mime_from_ext(Some("pages")),
        Some("application/vnd.apple.pages")
    );
}

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
    assert!(super::is_office_mime(
        "application/x-iwork-pages-sffpages"
    ));
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

#[test]
fn is_office_mime_text_plain_true() {
    assert!(super::is_office_mime("text/plain"));
}

#[test]
fn is_office_mime_text_markdown_true() {
    assert!(super::is_office_mime("text/markdown"));
}

#[test]
fn is_office_mime_text_rst_true() {
    assert!(super::is_office_mime("text/x-rst"));
}

#[test]
fn is_office_mime_text_csv_true() {
    assert!(super::is_office_mime("text/csv"));
}

#[test]
fn is_office_mime_text_tsv_true() {
    assert!(super::is_office_mime("text/tab-separated-values"));
}

#[test]
fn is_office_mime_image_false() {
    assert!(!super::is_office_mime("image/jpeg"));
}

#[test]
fn is_office_mime_video_false() {
    assert!(!super::is_office_mime("video/mp4"));
}

#[test]
fn is_office_mime_empty_false() {
    assert!(!super::is_office_mime(""));
}

// End-to-end：FakeBackend 喂内容不被 infer 识别（纯 ASCII txt） + path 带 `.txt`
// 扩展名 → Exif::open 走 `mime_from_ext` fallback 让 mime=text/plain，再分流到
// office::populate_office_dates（stub 阶段返 0 不改 exif）。覆盖 Exif::open 的
// `sniffed.is_empty()` 分支 true arm。
#[test]
fn open_uses_mime_from_ext_fallback_for_text_file() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/notes.txt"));
    fake.add_file(loc.clone(), b"plain text content\n".to_vec());

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "text/plain");
    // is_media() 仍 false：office 文档不算媒体，由 `--include-non-media` 解锁归档。
    assert!(!exif.is_media());
}

// 反向：sniffed 非空（infer 识别 JPEG）时 `Exif::open` 不走 ext fallback；JPEG
// path 即使带 `.txt` 扩展名（反逻辑场景）也按字节 mime 走，不被 ext 覆盖。覆盖
// `Exif::open` 的 `sniffed.is_empty()` 分支 false arm（infer 命中常规流程）。
// 该测试已由现有 `open_uses_*_fallback_for_*` 多个 e2e 隐式覆盖。
//
// 但 `path().extension()` 为 None 的兜底（loc 是无扩展名路径）专测一次。
#[test]
fn open_with_extensionless_path_and_unknown_bytes_yields_empty_mime() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/no_ext"));
    fake.add_file(loc.clone(), vec![0u8; 256]);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "");
    assert!(!exif.is_media());
}
