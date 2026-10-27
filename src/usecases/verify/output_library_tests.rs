use std::sync::{Arc, Barrier};

use camino::Utf8PathBuf;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::OutputLibrary;
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::Source;

fn local(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn info(backend: &Arc<FakeBackend>, path: &str, data: &[u8]) -> Info {
    let loc = local(path);
    backend.add_file(loc.clone(), data.to_vec());
    Info::open(&loc, backend.clone()).expect("internal: source info")
}

fn library(backend: &Arc<FakeBackend>) -> OutputLibrary {
    backend.add_dir(local("/out"));
    let source: Source = (local("/out"), backend.clone());
    OutputLibrary::from_source(&source)
}

#[test]
fn lazy_output_materializes_only_touched_size_bucket() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/a.jpg"), b"aa".to_vec());
    backend.add_file(local("/out/b.jpg"), b"bbb".to_vec());
    let source = info(&backend, "/src/source.jpg", b"zz");
    let library = library(&backend);
    assert!(!library.has_exact(&source));
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn lazy_output_prepare_exact_only_materializes_source_sizes() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/a.jpg"), b"aa".to_vec());
    backend.add_file(local("/out/b.jpg"), b"bbb".to_vec());
    let source = info(&backend, "/src/source.jpg", b"zz");
    let library = library(&backend);
    library.prepare_exact(std::slice::from_ref(&source));
    assert_eq!(library.materialized_bucket_count(), 1);
    assert!(!library.has_exact(&source));
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn lazy_output_finds_exact_across_different_names() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/archived.jpg"), b"same".to_vec());
    let source = info(&backend, "/src/source.jpg", b"same");
    let library = library(&backend);
    assert!(library.has_exact(&source));
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn lazy_output_ignores_unreadable_candidates() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/a.jpg"), b"same".to_vec());
    backend.inject_error(
        local("/out/a.jpg"),
        Op::OpenRead,
        std::io::ErrorKind::PermissionDenied,
    );
    let library = library(&backend);
    assert_eq!(library.candidates("a.jpg"), Vec::<Utf8PathBuf>::new());
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn lazy_output_empty_files_are_not_candidates() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/empty.jpg"), Vec::new());
    let library = library(&backend);
    assert_eq!(library.candidates("empty.jpg"), Vec::<Utf8PathBuf>::new());
    assert_eq!(library.materialized_bucket_count(), 0);
}

#[test]
fn output_library_unknown_candidate_and_file_output_return_empty() {
    let backend = Arc::new(FakeBackend::new("local"));
    let library = library(&backend);
    let missing = Utf8PathBuf::from("/out/missing.jpg");
    assert!(library.read_candidate(&missing, 100).is_none());
    let source: Source = (local("/out.txt"), backend);
    let file_library = OutputLibrary::from_source(&source);
    assert_eq!(
        file_library.candidates("out.txt"),
        Vec::<Utf8PathBuf>::new()
    );
    assert_eq!(file_library.materialized_bucket_count(), 0);
}

#[test]
fn lazy_output_concurrent_bucket_materializes_once() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/a.jpg"), b"aa".to_vec());
    backend.add_file(local("/out/b.jpg"), b"bb".to_vec());
    let source = info(&backend, "/src/source.jpg", b"zz");
    let library = Arc::new(library(&backend));
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let library = Arc::clone(&library);
                let source = &source;
                scope.spawn(move || library.has_exact(source))
            })
            .collect();
        for handle in handles {
            assert!(!handle.join().expect("internal: worker"));
        }
    });
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn lazy_output_rayon_materialization_completes() {
    let backend = Arc::new(FakeBackend::new("local"));
    backend.add_file(local("/out/a.jpg"), b"aa".to_vec());
    backend.add_file(local("/out/b.jpg"), b"bb".to_vec());
    let sources: Vec<_> = (0_u16..128)
        .map(|i| info(&backend, &format!("/src/{i}.jpg"), &i.to_le_bytes()))
        .collect();
    let library = library(&backend);
    let results: Vec<_> = sources
        .par_iter()
        .map(|src| library.has_exact(src))
        .collect();
    assert_eq!(results.len(), sources.len());
    assert_eq!(library.materialized_bucket_count(), 1);
}

#[test]
fn output_candidate_features_share_single_flight_result() {
    let backend = Arc::new(FakeBackend::new("local"));
    let bytes = std::fs::read("tests/data/sample-png-exif.png").unwrap();
    backend.add_file(local("/out/r.jpg"), bytes);
    let library = Arc::new(library(&backend));
    let barrier = Arc::new(Barrier::new(11));
    let path = Utf8PathBuf::from("/out/r.jpg");
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let library = Arc::clone(&library);
                let barrier = Arc::clone(&barrier);
                let path = &path;
                scope.spawn(move || {
                    barrier.wait();
                    library
                        .features(path, 1_000_000)
                        .expect("internal: features")
                })
            })
            .collect();
        barrier.wait();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("internal: worker"))
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|item| Arc::ptr_eq(&results[0], item)));
}
