//! Office 文档 e2e 归档测试：`sample-pdf-dated.pdf` + `sample-docx-dated.docx` 通过
//! `tidy(Commands::Copy { include_non_media: true, .. })` 落入 `{year}/{month}` 桶。
//! 验证 `office::populate_office_dates` 路由 → `Source::DocumentCreated` 候选 →
//! `Info::create_time` 一条路径在真实 `LocalBackend` 上跑通。

use tempfile::tempdir;
use tidymedia::{Commands, tidy};

use super::{DATA_DIR, local};

#[test]
fn copy_archives_pdf_by_dc_creation_date() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-pdf-dated.pdf"),
        src.path().join("sample-pdf-dated.pdf"),
    )
    .expect("seed pdf fixture");
    tidy(Commands::Copy {
        dry_run: false,
        include_non_media: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("copy with --include-non-media should succeed");
    // sample-pdf-dated.pdf 的 /CreationDate = 2017-02-14T10:30:00Z → 桶 2017/02
    let bucket = out.path().join("2017").join("02");
    assert!(
        bucket.exists(),
        "expected pdf to land in 2017/02 bucket; out tree: {:?}",
        walk(out.path())
    );
}

#[test]
fn copy_archives_docx_by_dcterms_created() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    std::fs::copy(
        format!("{DATA_DIR}/sample-docx-dated.docx"),
        src.path().join("sample-docx-dated.docx"),
    )
    .expect("seed docx fixture");
    tidy(Commands::Copy {
        dry_run: false,
        include_non_media: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("copy with --include-non-media should succeed");
    // sample-docx-dated.docx 的 dcterms:created = 2017-02-14T10:30:00Z → 桶 2017/02
    let bucket = out.path().join("2017").join("02");
    assert!(bucket.exists(), "expected docx to land in 2017/02 bucket");
}

#[test]
fn copy_archives_txt_falls_back_to_mtime_or_filename() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let txt_path = src.path().join("notes.txt");
    std::fs::write(&txt_path, b"hello world\n").unwrap();
    // mtime 设固定值让归档桶可预测：2020-06-15 → 桶 2020/06
    let target_mtime = filetime::FileTime::from_unix_time(1_592_179_200, 0);
    filetime::set_file_mtime(&txt_path, target_mtime).unwrap();

    tidy(Commands::Copy {
        dry_run: false,
        include_non_media: true,
        sources: vec![local(src.path().to_str().unwrap())],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .expect("copy with --include-non-media should succeed for txt");
    let bucket = out.path().join("2020").join("06");
    assert!(
        bucket.exists(),
        "txt with mtime 2020-06-15 should land in 2020/06"
    );
}

fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}
