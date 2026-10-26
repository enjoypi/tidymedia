use std::io::Cursor;

use super::riff_io::find_strd;
use super::tests_common::avif_with_make;
use super::tests_common::chunk;
use super::tests_common::list;
use super::tests_common::riff_avi;
use super::{FOURCC_HDRL, FOURCC_STRD};

#[test]
fn find_strd_returns_hdrl_body_containing_strd() {
    let hdrl = list(
        *FOURCC_HDRL,
        &chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")),
    );
    let body = find_strd(&mut Cursor::new(riff_avi(&hdrl))).expect("hdrl body");
    assert!(body.windows(4).any(|w| w == b"AVIF"));
}

#[test]
fn find_strd_rejects_non_avi() {
    assert_eq!(
        find_strd(&mut Cursor::new(b"RIFF\x04\x00\x00\x00WAVE".to_vec())),
        None
    );
}

#[test]
fn find_strd_truncated_header_returns_none() {
    assert_eq!(find_strd(&mut Cursor::new(b"RIFF".to_vec())), None);
}

#[test]
fn find_strd_without_hdrl_returns_none() {
    let junk = chunk(*b"JUNK", &[0u8; 4]);
    assert_eq!(find_strd(&mut Cursor::new(riff_avi(&junk))), None);
}
