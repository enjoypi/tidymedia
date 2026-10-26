use std::fs;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::tests_common::*;
use super::*;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

#[test]
fn do_copy_propagates_exists_error_when_indexed_file_deleted() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let indexed_path = dir.path().join("indexed.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&indexed_path, &a).unwrap();

    let src_path = dir.path().join("source.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&src_path, &b).unwrap();

    let mut info_src = Info::from(src_path.to_str().unwrap()).unwrap();
    info_src.set_exif(crate::entities::exif::Exif::with_mime("image/png"));

    let idx = crate::entities::file_index::Index::new();
    idx.insert(indexed_path.to_str().unwrap()).unwrap();
    fs::remove_file(&indexed_path).unwrap();

    let out_dir = local_loc(dir.path());
    let err = do_copy(
        &info_src,
        &out_dir,
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .unwrap_err();
    let _ = err;
}

#[test]
fn do_copy_remove_source_propagates_error() {
    use crate::adapters::backend::fake::{FakeBackend, Op};
    use std::io::ErrorKind;

    let be = Arc::new(FakeBackend::new("local"));
    let src = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    let dup = Location::Local(Utf8PathBuf::from("/out/photo.png"));
    be.add_file(src.clone(), b"same".to_vec());
    be.add_file(dup.clone(), b"same".to_vec());
    let backend: Arc<dyn Backend> = be.clone();
    let mut info = Info::open(&src, Arc::clone(&backend)).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    let dup_info = Info::open(&dup, backend).unwrap();
    let idx = crate::entities::file_index::Index::new();
    idx.add(dup_info);
    be.inject_error(src.clone(), Op::RemoveFile, ErrorKind::PermissionDenied);

    let opts = CopyOpts {
        dry_run: false,
        remove: true,
        include_non_media: false,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let err = do_copy(&info, &out, &(be.clone() as Arc<dyn Backend>), &idx, &opts).unwrap_err();

    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), ErrorKind::PermissionDenied);
    assert_eq!(be.read_bytes(&src), Some(b"same".to_vec()));
}

#[test]
fn do_copy_stream_remove_source_propagates_error() {
    use crate::adapters::backend::fake::{FakeBackend, Op};
    use std::io::ErrorKind;

    let be = Arc::new(FakeBackend::new("local"));
    let src = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    let dst = Location::Local(Utf8PathBuf::from("/out/1970/01/photo.png"));
    be.add_file(src.clone(), b"payload".to_vec());
    let backend: Arc<dyn Backend> = be.clone();
    let mut info = Info::open(&src, Arc::clone(&backend)).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    be.inject_error(src.clone(), Op::RemoveFile, ErrorKind::PermissionDenied);
    let opts = CopyOpts {
        dry_run: false,
        remove: true,
        include_non_media: false,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let idx = crate::entities::file_index::Index::new();
    let err = do_copy(&info, &out, &backend, &idx, &opts).unwrap_err();

    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), ErrorKind::PermissionDenied);
    assert_eq!(be.read_bytes(&src), Some(b"payload".to_vec()));
    assert_eq!(be.read_bytes(&dst), Some(b"payload".to_vec()));
}

#[test]
fn do_copy_create_dir_all_fails_when_path_blocked_by_file() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    fs::write(out.path().join("2024"), b"i am a file").unwrap();
    let idx = crate::entities::file_index::Index::new();
    let err = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    )
    .unwrap_err();
    let _ = err;
}

#[test]
#[cfg(unix)]
fn do_copy_file_copy_fails_when_source_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");

    let mut perms = fs::metadata(info.full_path.as_str()).unwrap().permissions();
    let original_mode = perms.mode();
    perms.set_mode(0o000);
    fs::set_permissions(info.full_path.as_str(), perms.clone()).unwrap();

    let out = tempdir().unwrap();
    let idx = crate::entities::file_index::Index::new();
    let res = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &idx,
        &default_opts(DEFAULT_TMPL),
    );

    perms.set_mode(original_mode);
    fs::set_permissions(info.full_path.as_str(), perms).unwrap();

    assert!(res.is_err(), "expected copy failure but got {res:?}");
}

#[test]
#[cfg(unix)]
fn do_copy_move_fails_when_src_parent_locked() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let src_parent = root.path().join("locked_src");
    fs::create_dir(&src_parent).unwrap();
    let info = make_media_info(&src_parent, "photo.png");

    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();
    let idx = crate::entities::file_index::Index::new();

    let mut perms = fs::metadata(&src_parent).unwrap().permissions();
    let original_mode = perms.mode();
    perms.set_mode(0o555);
    fs::set_permissions(&src_parent, perms.clone()).unwrap();

    let opts = CopyOpts {
        dry_run: false,
        remove: true,
        include_non_media: false,
        doc_only: false,
        template: DEFAULT_TMPL,
    };
    let res = do_copy(&info, &local_loc(&out), &local_arc(), &idx, &opts);

    perms.set_mode(original_mode);
    fs::set_permissions(&src_parent, perms).unwrap();

    assert!(res.is_err(), "expected rename failure but got {res:?}");
}

#[test]
fn do_copy_retries_next_name_when_write_hits_already_exists() {
    use crate::adapters::backend::fake::{FakeBackend, Op};

    let be = Arc::new(FakeBackend::new("local"));
    let src = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    be.add_file(src.clone(), b"payload".to_vec());
    let backend: Arc<dyn Backend> = be.clone();
    let mut info = Info::open(&src, Arc::clone(&backend)).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    let out = Location::Local(Utf8PathBuf::from("/out"));
    be.inject_error(
        out.join_path("photo.png"),
        Op::OpenWrite,
        std::io::ErrorKind::AlreadyExists,
    );

    let idx = crate::entities::file_index::Index::new();
    let copied = do_copy(
        &info,
        &out,
        &backend,
        &idx,
        &default_opts("{valuable_name}"),
    )
    .unwrap();

    assert!(copied);
    assert_eq!(
        be.read_bytes(&out.join_path("photo_1.png")),
        Some(b"payload".to_vec())
    );
    assert_eq!(be.read_bytes(&out.join_path("photo.png")), None);
}

#[test]
fn do_copy_dry_run_propagates_hash_read_error() {
    use crate::adapters::backend::fake::FakeBackend;

    let be = Arc::new(FakeBackend::new("local"));
    let src = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    be.add_file(src.clone(), b"payload".to_vec());
    let backend: Arc<dyn Backend> = be.clone();
    let mut info = Info::open(&src, Arc::clone(&backend)).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    be.inject_reader_error(src, std::io::ErrorKind::PermissionDenied);

    let mut opts = default_opts("{valuable_name}");
    opts.dry_run = true;
    let idx = crate::entities::file_index::Index::new();
    let out = Location::Local(Utf8PathBuf::from("/out"));
    let err = do_copy(&info, &out, &backend, &idx, &opts).unwrap_err();

    let crate::entities::common::Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
}
