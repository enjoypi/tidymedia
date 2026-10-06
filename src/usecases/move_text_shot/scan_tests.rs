use super::drain_reader_to_option;
use crate::entities::test_common::FlakyIo;

#[test]
fn drain_reader_to_option_propagates_read_error() {
    let mut io = FlakyIo::new(vec![1, 2, 3], 0, 0);
    assert!(drain_reader_to_option(&mut io, Vec::new()).is_err());
}
