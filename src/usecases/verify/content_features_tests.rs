use super::{CandidateFeatures, PreparedSource, rotated_candidate_similar};

fn png() -> Vec<u8> {
    std::fs::read("tests/data/sample-png-exif.png").expect("internal: png fixture")
}

#[test]
fn rotated_candidate_rejects_invalid_source_image() {
    let candidate = CandidateFeatures::from_bytes(&png(), "png");
    let source = PreparedSource::new(b"not-an-image");
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(png())
    }));
}

#[test]
fn rotated_candidate_propagates_l4_read_failure() {
    let bytes = png();
    let candidate = CandidateFeatures::from_bytes(&bytes, "png");
    let source = PreparedSource::new(&bytes);
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        None
    }));
}

#[test]
fn rotated_candidate_rejects_invalid_l4_bytes() {
    let bytes = png();
    let candidate = CandidateFeatures::from_bytes(&bytes, "png");
    let source = PreparedSource::new(&bytes);
    assert!(!rotated_candidate_similar(&source, &candidate, 20, &|| {
        Some(b"changed-before-l4".to_vec())
    }));
}
