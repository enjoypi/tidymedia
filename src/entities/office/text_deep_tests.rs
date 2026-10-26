use std::io::Cursor;

use super::extract_text as text_extract_text;
use crate::entities::office::tests_common::FailReader;

#[test]
fn text_extract_text_happy_and_read_error() {
    let mut reader = Cursor::new(b"hello world".to_vec());
    assert_eq!(
        text_extract_text(&mut reader, "text/plain", 64),
        "hello world"
    );
    let mut failing = FailReader;
    assert_eq!(text_extract_text(&mut failing, "text/plain", 64), "");
}
