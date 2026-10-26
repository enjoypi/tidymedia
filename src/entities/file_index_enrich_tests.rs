use std::fs;

use tempfile::tempdir;

use super::super::file_info;
use super::super::test_common as common;
use super::Index;

#[test]
fn parse_exif() {
    let mut index = Index::new();
    index.visit_dir(common::DATA_DIR);
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, false);

    let jpeg_path = file_info::full_path(common::DATA_JPEG_WITH_EXIF).unwrap();
    let entry = index.get(jpeg_path.as_path()).unwrap();
    let info = entry.value();
    let exif = info.exif().unwrap();
    assert_eq!(exif.mime_type(), "image/jpeg");
    assert!(exif.is_media());
    assert_eq!(exif.date_time_original(), 1_704_110_400);
}

#[test]
fn parse_exif_empty_index_ok() {
    let mut index = Index::new();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, false);
    assert_eq!(index.len(), 0);
}

#[test]
fn parse_exif_skips_files_deleted_between_visit_and_parse() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ghost.bin");
    fs::write(&path, [0xFFu8; 32]).unwrap();
    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());
    assert_eq!(index.len(), 1);
    fs::remove_file(&path).unwrap();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, false);
    assert_eq!(index.len(), 1);
}

#[test]
fn parse_exif_skips_file_when_open_read_fails() {
    use std::sync::Arc;

    use crate::adapters::backend::fake::{FakeBackend, Op};
    use crate::entities::uri::Location;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/broken.txt"));
    fake.add_file(loc.clone(), b"some text body".to_vec());
    let backend: Arc<dyn crate::entities::backend::Backend> = Arc::clone(&fake) as _;

    let mut index = Index::new();
    index.visit_location(&loc, &backend);
    fake.inject_error(loc.clone(), Op::OpenRead, std::io::ErrorKind::TimedOut);
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, false);

    let key = camino::Utf8PathBuf::from(loc.display());
    let entry = index.get(&key).unwrap();
    assert!(
        entry.value().exif_ref().is_none(),
        "open_read Err must leave Info without exif"
    );
}

#[test]
fn enrich_candidates_adds_non_empty_candidates() {
    use std::sync::Arc;

    use crate::entities::backend::Backend;
    use crate::entities::media_time::{Candidate, Source};
    use crate::entities::uri::Location;

    fn one_candidate(_: &Location, _: &Arc<dyn Backend>) -> Vec<Candidate> {
        vec![Candidate {
            utc: chrono::DateTime::from_timestamp(1_487_068_200, 0).unwrap(),
            offset: None,
            source: Source::XmpSidecar,
            inferred_offset: false,
        }]
    }

    let dir = tempdir().unwrap();
    let png = common::copy_png_to(dir.path(), "photo.png").unwrap();
    let mut index = Index::new();
    index.insert(png.to_str().unwrap()).unwrap();
    index.enrich_candidates(one_candidate, &|_| true);

    let key = camino::Utf8PathBuf::from(png.to_str().unwrap());
    let entry = index.get(&key).unwrap();
    let t = entry
        .value()
        .create_time(0, chrono::FixedOffset::east_opt(0).unwrap());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 1_487_068_200);
}

#[test]
fn enrich_candidates_skips_files_rejected_by_keep() {
    use std::sync::Arc;

    use crate::entities::backend::Backend;
    use crate::entities::media_time::{Candidate, Source};
    use crate::entities::uri::Location;

    fn one_candidate(_: &Location, _: &Arc<dyn Backend>) -> Vec<Candidate> {
        vec![Candidate {
            utc: chrono::DateTime::from_timestamp(1_487_068_200, 0).unwrap(),
            offset: None,
            source: Source::XmpSidecar,
            inferred_offset: false,
        }]
    }

    let dir = tempdir().unwrap();
    let png = common::copy_png_to(dir.path(), "photo.png").unwrap();
    let mut index = Index::new();
    index.insert(png.to_str().unwrap()).unwrap();
    index.enrich_candidates(one_candidate, &|_| false);

    let key = camino::Utf8PathBuf::from(png.to_str().unwrap());
    let entry = index.get(&key).unwrap();
    let t = entry
        .value()
        .create_time(0, chrono::FixedOffset::east_opt(0).unwrap());
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(
        secs,
        u64::try_from(crate::entities::test_common::FIXED_MEDIA_MTIME).unwrap()
    );
}

#[test]
fn classify_documents_writes_back_category_for_office_files() {
    use crate::entities::file_index::TextClassifyProvider;

    let dir = tempdir().unwrap();
    let txt = dir.path().join("notes.txt");
    fs::write(&txt, b"invoice body text").unwrap();
    let mut index = Index::new();
    index.insert(txt.to_str().unwrap()).unwrap();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, true);

    let provider: TextClassifyProvider =
        Box::new(|_loc, _backend, mime| (mime == "text/plain").then(|| "invoice".to_string()));
    index.classify_documents(&provider);

    let key = camino::Utf8PathBuf::from(txt.to_str().unwrap());
    let entry = index.get(&key).unwrap();
    assert_eq!(entry.value().category_ref(), Some("invoice"));
}

#[test]
fn classify_documents_skips_non_office_files() {
    use crate::entities::file_index::TextClassifyProvider;

    let dir = tempdir().unwrap();
    let png = common::copy_png_to(dir.path(), "photo.png").unwrap();
    let mut index = Index::new();
    index.insert(png.to_str().unwrap()).unwrap();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, false);

    let provider: TextClassifyProvider = Box::new(|_, _, _| Some("should-not-happen".to_string()));
    index.classify_documents(&provider);

    let key = camino::Utf8PathBuf::from(png.to_str().unwrap());
    let entry = index.get(&key).unwrap();
    assert!(entry.value().category_ref().is_none());
}

#[test]
fn classify_documents_none_leaves_category_unset() {
    use crate::entities::file_index::TextClassifyProvider;

    let dir = tempdir().unwrap();
    let txt = dir.path().join("plain.txt");
    fs::write(&txt, b"nothing matches").unwrap();
    let mut index = Index::new();
    index.insert(txt.to_str().unwrap()).unwrap();
    index.parse_exif(chrono::FixedOffset::east_opt(0).unwrap(), true, true);

    let provider: TextClassifyProvider = Box::new(|_, _, _| None);
    index.classify_documents(&provider);

    let key = camino::Utf8PathBuf::from(txt.to_str().unwrap());
    let entry = index.get(&key).unwrap();
    assert!(entry.value().category_ref().is_none());
}
