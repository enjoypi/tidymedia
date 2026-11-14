use std::io::{Cursor, Read, Seek, SeekFrom};

use super::*;

fn budget(capacity: u64) -> &'static Budget {
    Box::leak(Box::new(Budget::new(Level::Work, capacity)))
}

fn reader(len: usize, work: &'static Budget, free_bytes: u64, max_bytes: u64) -> MeteredReader {
    MeteredReader::new(
        Box::new(Cursor::new(vec![7_u8; len])),
        ParseLimits {
            work,
            free_bytes,
            max_bytes,
        },
    )
}

#[test]
fn small_reads_stay_within_the_free_allowance() {
    let work = budget(300);
    let mut metered = reader(8, work, 16, 100);
    let mut buf = [0_u8; 8];
    assert_eq!(metered.read(&mut buf).unwrap(), 8);
    assert!(work.reserve(300).is_ok());
}

#[test]
fn crossing_the_free_allowance_reserves_work_once_until_dropped() {
    let work = budget(300);
    let mut metered = reader(64, work, 16, 100);
    let mut buf = [0_u8; 20];
    assert_eq!(metered.read(&mut buf).unwrap(), 20);
    assert_eq!(metered.read(&mut buf).unwrap(), 20);
    assert_eq!(metered.seek(SeekFrom::Start(0)).unwrap(), 0);
    drop(metered);
    assert!(work.reserve(300).is_ok());
}

#[test]
fn exceeding_the_maximum_fails_the_read() {
    let mut metered = reader(64, budget(300), 16, 30);
    let mut buf = [0_u8; 20];
    assert_eq!(metered.read(&mut buf).unwrap(), 20);
    let err = metered.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
}

#[test]
fn budget_smaller_than_the_reservation_fails_the_read() {
    let mut metered = reader(64, budget(299), 16, 100);
    let mut buf = [0_u8; 20];
    let err = metered.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
}

#[test]
fn default_limits_apply_until_installed() {
    let limits = parse_limits();
    assert!(limits.free_bytes <= limits.max_bytes);
    assert!(limits.work.capacity() >= limits.max_bytes * 3);
}

#[derive(Debug)]
struct Broken;

impl Read for Broken {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("inject"))
    }
}

impl Seek for Broken {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

#[test]
fn inner_read_errors_propagate() {
    let mut metered = MeteredReader::new(
        Box::new(Broken),
        ParseLimits {
            work: budget(1),
            free_bytes: 1,
            max_bytes: 1,
        },
    );
    let mut buf = [0_u8; 4];
    assert_eq!(metered.read(&mut buf).unwrap_err().to_string(), "inject");
}
