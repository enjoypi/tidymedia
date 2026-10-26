use std::io::Cursor;

use super::*;

#[test]
fn extract_text_reads_body() {
    let mut r = Cursor::new(b"raw body text".to_vec());
    assert_eq!(extract_text(&mut r, "text/plain", 64), "raw body text");
}

#[test]
fn extract_text_truncates_to_max_bytes() {
    let mut r = Cursor::new(b"0123456789".to_vec());
    assert_eq!(extract_text(&mut r, "text/plain", 4), "0123");
}

#[test]
fn extract_text_read_error_returns_empty() {
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; 8], 0, 0);
    assert_eq!(extract_text(&mut io, "text/plain", 64), "");
}
