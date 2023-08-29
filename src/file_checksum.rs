use std::io::{Error, ErrorKind, Read};
use std::path::Path;
use std::{fs, io};

use generic_array::GenericArray;
use memmap2::Mmap;
use sha2::{Digest, Sha512};

use super::SecureChecksum;

pub const FAST_READ_SIZE: usize = 4096;

#[derive(Debug, Clone)]
pub struct FileChecksum {
    pub short: u64,
    pub full: u64,
    pub path: String,
    pub secure: SecureChecksum,
    pub size: u64,

    pub bytes_read: u64,
    true_full: bool,
}

impl FileChecksum {
    pub fn new_path(path: &Path) -> io::Result<Self> {
        let meta = path.metadata()?;
        if !meta.is_file() {
            return Err(Error::new(
                ErrorKind::Other,
                format!("{} is a directory", path.display()),
            ));
        }

        if meta.len() == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{} is empty", path.display()),
            ));
        }

        let p = path.canonicalize()?;
        let p = match p.to_str() {
            Some(s) => s,
            None => {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("invalid filename {}", path.display()),
                ))
            }
        };
        let p = p.strip_prefix("\\\\?\\").unwrap_or(p);

        let (bytes_read, short, full) = fast_checksum(p)?;

        Ok(Self {
            short,
            full,
            secure: GenericArray::default(),
            path: p.to_string(),
            size: meta.len(),
            bytes_read: bytes_read as u64,
            true_full: meta.len() <= FAST_READ_SIZE as u64,
        })
    }

    pub fn new(path: &str) -> io::Result<Self> {
        let path = fs::canonicalize(path)?;

        Self::new_path(path.as_path())
    }

    pub async fn new_async(path: &Path) -> io::Result<Self> {
        Self::new_path(path)
    }

    pub fn calc_full(&mut self) -> io::Result<u64> {
        if self.true_full {
            return Ok(self.full);
        }

        let (bytes_read, full) = full_checksum(self.path.as_str())?;
        self.bytes_read += bytes_read as u64;
        self.full = full;
        self.true_full = true;

        Ok(full)
    }

    pub fn calc_secure(&mut self) -> io::Result<SecureChecksum> {
        if self.secure != GenericArray::default() {
            return Ok(self.secure);
        }

        let (bytes_read, secure) = secure_checksum(self.path.as_str())?;
        self.bytes_read += bytes_read as u64;
        self.secure = secure;

        Ok(self.secure)
    }
}

impl PartialEq for FileChecksum {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.short == other.short && self.full == other.full
    }
}

fn fast_checksum(path: &str) -> io::Result<(usize, u64, u64)> {
    let mut file = fs::File::open(path)?;

    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file.read(&mut buffer)?;

    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));

    Ok((bytes_read, short, full))
}

fn full_checksum(path: &str) -> io::Result<(usize, u64)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    Ok((mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap)))
}

fn secure_checksum(path: &str) -> io::Result<(usize, SecureChecksum)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok((mmap.len(), Sha512::digest(&mmap)))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek};
    use std::{fs, io};

    use generic_array::GenericArray;
    use sha2::Digest;
    use wyhash;
    use xxhash_rust::xxh3;

    use crate::tests;

    use super::FileChecksum;

    struct ChecksumTest {
        short_wyhash: u64,
        short_xxhash: u64,
        short_read: usize,
        full: u64,
        file_size: usize,

        secure: super::SecureChecksum,
    }

    impl ChecksumTest {
        fn new(path: &str) -> io::Result<ChecksumTest> {
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

            Ok(ChecksumTest {
                short_wyhash,
                short_xxhash,
                short_read,
                full,
                file_size,
                secure,
            })
        }
    }

    #[test]
    fn small_file() -> tests::Result {
        let ct = ChecksumTest::new(tests::DATA_SMALL)?;
        assert_eq!(ct.short_wyhash, tests::DATA_SMALL_WYHASH);
        assert_eq!(ct.short_xxhash, tests::DATA_SMALL_XXHASH);
        assert!(ct.file_size <= super::FAST_READ_SIZE);
        assert_eq!(ct.short_read, ct.file_size);
        assert_eq!(ct.short_xxhash, ct.full);
        assert_eq!(ct.secure, tests::data_small_sha512());

        let mut f = FileChecksum::new(tests::DATA_SMALL)?;
        assert_eq!(f.short, ct.short_wyhash);
        assert_eq!(f.full, ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full()?, ct.full);
        assert_eq!(f.full, ct.full);
        assert_eq!(f.secure, GenericArray::default());
        assert_eq!(f.calc_secure()?, tests::data_small_sha512());
        assert_eq!(f.secure, tests::data_small_sha512());

        Ok(())
    }

    #[test]
    fn large_file() -> tests::Result {
        let ct = ChecksumTest::new(tests::DATA_LARGE)?;
        assert_eq!(ct.short_wyhash, tests::DATA_LARGE_WYHASH);
        assert_ne!(ct.short_xxhash, tests::DATA_LARGE_XXHASH);
        assert_eq!(ct.short_read, super::FAST_READ_SIZE);
        assert!(ct.short_read < ct.file_size);
        assert_eq!(ct.full, tests::DATA_LARGE_XXHASH);
        assert_eq!(ct.secure, tests::data_large_sha512());

        let mut f = FileChecksum::new(tests::DATA_LARGE)?;
        assert_eq!(f.short, ct.short_wyhash);
        assert_eq!(f.full, ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full()?, ct.full);
        assert_eq!(f.full, ct.full);
        assert_eq!(f.secure, GenericArray::default());
        assert_eq!(f.calc_secure()?, tests::data_large_sha512());
        assert_eq!(f.secure, tests::data_large_sha512());

        Ok(())
    }

    #[test]
    fn bytes_read() -> tests::Result {
        let meta = fs::metadata(tests::DATA_LARGE)?;

        {
            let (bytes_read, _fast, _full) = super::fast_checksum(tests::DATA_LARGE)?;
            assert_eq!(bytes_read, super::FAST_READ_SIZE);

            let (bytes_read, full) = super::full_checksum(tests::DATA_LARGE)?;
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(full, tests::DATA_LARGE_XXHASH);
        }

        let mut checksum = super::FileChecksum::new(tests::DATA_LARGE)?;
        assert_eq!(checksum.bytes_read, super::FAST_READ_SIZE as u64);
        assert_eq!(checksum.calc_full()?, tests::DATA_LARGE_XXHASH);
        assert_eq!(
            checksum.bytes_read,
            super::FAST_READ_SIZE as u64 + meta.len()
        );
        // no read file when twice
        assert_eq!(checksum.calc_full()?, tests::DATA_LARGE_XXHASH);
        assert_eq!(
            checksum.bytes_read,
            super::FAST_READ_SIZE as u64 + meta.len()
        );

        assert_eq!(checksum.calc_secure()?, tests::data_large_sha512());
        assert_eq!(
            checksum.bytes_read,
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        // no read file when twice
        assert_eq!(checksum.calc_secure()?, tests::data_large_sha512());
        assert_eq!(
            checksum.bytes_read,
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        Ok(())
    }
}
