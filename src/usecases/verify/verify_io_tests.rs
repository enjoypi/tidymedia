use std::io::{Cursor, Read};

use super::read_stream;

#[test]
fn read_stream_accepts_partial_tsv() {
    let input = Cursor::new(b"/a.jpg\t2024:01".to_vec());
    assert_eq!(read_stream(input, 64).unwrap(), "/a.jpg\t2024:01");
}

#[test]
fn read_stream_rejects_invalid_utf8() {
    assert_eq!(
        read_stream(Cursor::new([0xff]), 64).unwrap_err().kind(),
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
        read_stream(Failing, 64).unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
}

#[test]
fn read_stream_rejects_input_beyond_cap() {
    assert_eq!(
        read_stream(Cursor::new(b"12345".to_vec()), 4)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::FileTooLarge
    );
    assert_eq!(
        read_stream(Cursor::new(b"1234".to_vec()), 4).unwrap(),
        "1234"
    );
}
