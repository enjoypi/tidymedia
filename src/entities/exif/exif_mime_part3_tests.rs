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

// `open_filtered(parse_non_media=false)` 对非媒体 MIME 在 sniff 后短路：返回
// mime-only Exif（`is_media` 判定仍可用），跳过整文件容器解析——copy/move 在
// `--include-non-media` 未开时非媒体解析纯属浪费。
#[test]
fn open_filtered_skips_parsing_for_non_media_mime() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/notes.txt"));
    fake.add_file(loc.clone(), b"plain text content\n".to_vec());

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open_filtered(&loc, &backend, utc(), false, false).unwrap();
    assert_eq!(exif.mime_type(), "text/plain");
    assert!(!exif.is_media());
    assert_eq!(exif.doc_created(), 0);
}

// 反向：媒体 MIME 在 `parse_non_media=false` 下不短路，正常走容器解析
// （`!is_media_mime` 子条件 false arm）。
#[test]
fn open_filtered_still_parses_media_mime() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let png = std::fs::read(crate::entities::test_common::DATA_DNS_BENCHMARK).unwrap();
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/photo.png"));
    fake.add_file(loc.clone(), png);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open_filtered(&loc, &backend, utc(), false, false).unwrap();
    assert!(exif.is_media());
    assert!(exif.mime_type().starts_with("image/"));
}

// `open_filtered(doc_only=true)` 对文档族 MIME 整文件解析：PDF 的 /CreationDate
// 被提取（doc_created 非 0），证明未走 mime-only 短路。
#[test]
fn open_filtered_doc_only_parses_office_mime() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let pdf = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sample-pdf-dated.pdf"
    ))
    .unwrap();
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/doc.pdf"));
    fake.add_file(loc.clone(), pdf);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open_filtered(&loc, &backend, utc(), false, true).unwrap();
    assert!(exif.is_office());
    assert_ne!(exif.doc_created(), 0);
}

// `open_filtered(doc_only=true)` 对媒体 MIME 反向短路：mime-only Exif
// （is_media 判定可用），跳过 EXIF 容器解析——copy-doc 会 skip 媒体，解析纯属浪费。
// fixture 带 DateTimeOriginal：若未短路 date_time_original 必非 0，断言有区分度。
#[test]
fn open_filtered_doc_only_skips_media_mime() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let jpg = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sample-with-exif.jpg"
    ))
    .unwrap();
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/photo.jpg"));
    fake.add_file(loc.clone(), jpg);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open_filtered(&loc, &backend, utc(), true, true).unwrap();
    assert!(exif.is_media());
    assert!(!exif.is_office());
    assert_eq!(exif.date_time_original(), 0);
}

// CFB fixture（真实 sample-doc-dated.doc）在 256 字节 sniff 窗口下被 infer 识别为
// 泛化 `application/x-ole-storage`（CLSID 细分需完整 header 必失败）→ 扩展名重映射
// 到 application/msword → CFB 路由命中 SummaryInformation 时间。回归钉死本次
// e2e 矩阵抓到的重映射缺口。
#[test]
fn open_filtered_remaps_ole_storage_by_extension() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let doc = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sample-doc-dated.doc"
    ))
    .unwrap();
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/contract.doc"));
    fake.add_file(loc.clone(), doc);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open_filtered(&loc, &backend, utc(), false, true).unwrap();
    assert_eq!(exif.mime_type(), "application/msword");
    assert_ne!(exif.doc_created(), 0);
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
