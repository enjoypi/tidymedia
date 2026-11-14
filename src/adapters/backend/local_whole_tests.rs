use std::io;
use std::path::Path;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::map_local;
use crate::adapters::backend::local::LocalBackend;
use crate::adapters::backend::local::mmap_io::map_path;
use crate::entities::backend::{FileBytes, MappedFile};
use crate::entities::uri::Location;

fn fixed(_: &Path) -> bool {
    true
}

fn removable(_: &Path) -> bool {
    false
}

fn refuse(_: &Path) -> io::Result<Option<MappedFile>> {
    Err(io::Error::other("inject"))
}

#[test]
fn non_fixed_volume_falls_back_to_streaming_without_mapping() {
    assert!(
        map_local(Path::new("/x"), removable, refuse)
            .unwrap()
            .is_none()
    );
}

#[test]
fn fixed_volume_maps_and_propagates_mapper_error() {
    assert!(map_local(Path::new("/x"), fixed, refuse).is_err());
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, b"hello").unwrap();
    let mapped = map_local(&path, fixed, map_path).unwrap().unwrap();
    let bytes = FileBytes::Mapped(mapped);
    assert_eq!(&*bytes, b"hello");
    assert!(bytes.unchanged());
    std::io::Write::write_all(
        &mut std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap(),
        b" world",
    )
    .unwrap();
    assert!(!bytes.unchanged());
}

#[test]
fn empty_and_missing_files_are_not_mapped() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty.bin");
    std::fs::write(&empty, b"").unwrap();
    assert!(map_local(&empty, fixed, map_path).unwrap().is_none());
    assert!(map_local(&dir.path().join("missing"), fixed, map_path).is_err());
}

#[test]
fn local_backend_maps_local_paths_and_rejects_other_schemes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, b"payload").unwrap();
    let backend = LocalBackend::arc();
    let loc = Location::Local(Utf8PathBuf::from(path.to_str().unwrap()));
    if let Some(mapped) = backend.map_whole(&loc).unwrap() {
        assert_eq!(&*FileBytes::Mapped(mapped), b"payload");
    }
    let smb = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a.bin"),
    };
    let err = backend.map_whole(&smb).err().unwrap();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
