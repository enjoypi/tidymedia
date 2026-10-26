use std::io::Write;

use super::*;

#[test]
fn inflate_capped_valid_zlib_roundtrip() {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(b"payload").unwrap();
    let z = enc.finish().unwrap();
    assert_eq!(inflate_capped(&z).unwrap(), b"payload");
}

#[test]
fn inflate_capped_garbage_returns_none() {
    assert!(inflate_capped(b"definitely not zlib").is_none());
}

#[test]
fn inflate_capped_empty_output_returns_none() {
    let enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let z = enc.finish().unwrap();
    assert!(inflate_capped(&z).is_none());
}
