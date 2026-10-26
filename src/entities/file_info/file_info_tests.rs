use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;

use sha2::Digest;
use wyhash;
use xxhash_rust::xxh3;

use super::super::test_common as common;
use super::Info;

struct HashTest {
    short_wyhash: u64,
    short_xxhash: u64,
    short_read: usize,
    full: u64,
    file_size: usize,

    secure: super::SecureHash,
}

impl HashTest {
    fn new(path: &str) -> io::Result<HashTest> {
        let mut file = fs::File::open(path)?;

        let mut buffer = [0; super::FAST_READ_SIZE];
        let short_read = file.read(&mut buffer)?;
        if short_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "File is empty",
            ));
        }

        let short_wyhash = wyhash::wyhash(&(buffer[..short_read]), 0);
        let short_xxhash = xxh3::xxh3_64(&(buffer[..short_read]));

        let mut buffer = Vec::new();
        file.seek(std::io::SeekFrom::Start(0))?;
        let file_size = file.read_to_end(&mut buffer)?;
        let full = xxh3::xxh3_64(buffer.as_slice());

        let mut hasher = sha2::Sha512::new();
        hasher.update(buffer.as_slice());
        let secure = hasher.finalize();

        Ok(HashTest {
            short_wyhash,
            short_xxhash,
            short_read,
            full,
            file_size,
            secure,
        })
    }
}

include!("file_info_part1_tests.rs");
include!("file_info_part2_tests.rs");
