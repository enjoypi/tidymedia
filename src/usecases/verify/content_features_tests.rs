use super::{CandidateFeatures, PreparedSource, rotated_candidate_similar};
use crate::entities::backend::{FileBytes, MappedBytes, MappedFile};
use crate::usecases::image_input::DecodeCtx;

struct Changed(Vec<u8>);

impl MappedBytes for Changed {
    fn bytes(&self) -> &[u8] {
        &self.0
    }

    fn unchanged(&self) -> bool {
        false
    }
}

#[test]
fn rotated_candidate_rejects_bytes_changed_during_second_read() {
    let bytes = png();
    let candidate = CandidateFeatures::from_bytes(&bytes, "png", DecodeCtx::global());
    let source = PreparedSource::new(&bytes, DecodeCtx::global());
    assert!(rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(FileBytes::Heap(png()))
    }));
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(FileBytes::Mapped(MappedFile::new(Box::new(Changed(png())))))
    }));
}

fn png() -> Vec<u8> {
    std::fs::read("tests/data/sample-png-exif.png").expect("internal: png fixture")
}

#[test]
fn rotated_candidate_rejects_invalid_source_image() {
    let candidate = CandidateFeatures::from_bytes(&png(), "png", DecodeCtx::global());
    let source = PreparedSource::new(b"not-an-image", DecodeCtx::global());
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(FileBytes::Heap(png()))
    }));
}

#[test]
fn rotated_candidate_propagates_l4_read_failure() {
    let bytes = png();
    let candidate = CandidateFeatures::from_bytes(&bytes, "png", DecodeCtx::global());
    let source = PreparedSource::new(&bytes, DecodeCtx::global());
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        None
    }));
}

#[test]
fn rotated_candidate_rejects_invalid_l4_bytes() {
    let bytes = png();
    let candidate = CandidateFeatures::from_bytes(&bytes, "png", DecodeCtx::global());
    let source = PreparedSource::new(&bytes, DecodeCtx::global());
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(FileBytes::Heap(b"changed-before-l4".to_vec()))
    }));
}

#[test]
fn prepared_source_image_is_decoded_once() {
    let bytes = png();
    let source = PreparedSource::new(&bytes, DecodeCtx::global());
    assert!(!source.too_large());
    let first = std::ptr::from_ref(source.image());
    let second = std::ptr::from_ref(source.image());
    assert_eq!(first, second);
    assert!(!source.too_large());
    assert!(source.image().ready().is_some());
}
