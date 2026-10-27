use std::io::{Cursor, Read};

use super::read_stream;

#[test]
fn read_stream_accepts_partial_tsv() {
    let input = Cursor::new(b"/a.jpg\t2024:01".to_vec());
    assert_eq!(read_stream(input).unwrap(), "/a.jpg\t2024:01");
}

#[test]
fn read_stream_rejects_invalid_utf8() {
    assert_eq!(
        read_stream(Cursor::new([0xff])).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn read_stream_propagates_read_error() {
    struct Failing;
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read failed"))
        }
    }
    assert_eq!(
        read_stream(Failing).unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
}
