use std::hash::Hasher;
use std::io::{Error, ErrorKind, Read};
use std::{fs, io};

use generic_array::GenericArray;
use sha2::{Digest, Sha512};

use super::SecureChecksum;

#[derive(Debug)]
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
    pub fn new(path: &str) -> io::Result<Self> {
        let path = fs::canonicalize(path)?;
        let meta = path.metadata()?;
        if !meta.is_file() {
            return Err(Error::from(ErrorKind::Unsupported));
        }

        if meta.len() == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "empty file"));
        }

        let path = match path.to_str() {
            Some(s) => String::from(s),
            None => return Err(Error::from(ErrorKind::Unsupported)),
        };

        let (bytes_read, short, full) = fast_checksum(&path)?;

        Ok(Self {
            short,
            full,
            secure: GenericArray::default(),
            path,
            size: meta.len(),
            bytes_read: bytes_read as u64,
            true_full: meta.len() <= super::READ_BUFFER_SIZE as u64,
        })
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

    // pub fn equal(&mut self, other: &mut Self) -> bool {
    //     if self != other {
    //         return false;
    //     }
    //
    //     if self.secure == GenericArray::default() && self.calc_secure().is_err() {
    //         return false;
    //     }
    //
    //     if other.secure == GenericArray::default() && other.calc_secure().is_err() {
    //         return false;
    //     }
    //
    //     self.secure == other.secure
    // }
}

impl PartialEq for FileChecksum {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.short == other.short && self.full == other.full
    }
}

fn fast_checksum(path: &str) -> io::Result<(usize, u64, u64)> {
    let mut file = fs::File::open(path)?;

    let mut buffer = [0; super::READ_BUFFER_SIZE];
    let bytes_read = file.read(&mut buffer)?;

    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));

    Ok((bytes_read, short, full))
}

fn full_checksum(path: &str) -> io::Result<(usize, u64)> {
    let mut file = fs::File::open(path)?;
    let mut long_hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buffer: [u8; super::READ_BUFFER_SIZE] = [0; super::READ_BUFFER_SIZE];

    let mut total_read = 0;
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read > 0 {
            total_read += bytes_read;
            long_hasher.update(&buffer[..bytes_read]);
        } else {
            break;
        }
    }

    Ok((total_read, long_hasher.finish()))
}

fn secure_checksum(path: &str) -> io::Result<(usize, SecureChecksum)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha512::new();
    let mut buffer: [u8; super::READ_BUFFER_SIZE] = [0; super::READ_BUFFER_SIZE];

    let mut total_read = 0;
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read > 0 {
            total_read += bytes_read;
            hasher.update(&buffer[..bytes_read]);
        } else {
            break;
        }
    }

    Ok((total_read, hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    #[test]
    fn bytes_read() -> std::result::Result<(), Box<dyn std::error::Error>> {
        const XXHASH_V: u64 = 0xdce253cfb92205e2;

        let home = env::var("HOME")?;
        let filename = home + "/Movies/桥水基金中国区总裁王沿：全天候的投资原则.mp4";
        let meta = fs::metadata(filename.as_str())?;

        {
            let (bytes_read, _fast, _full) = super::fast_checksum(filename.as_str())?;
            assert_eq!(bytes_read, super::super::READ_BUFFER_SIZE);

            let (bytes_read, full) = super::full_checksum(filename.as_str())?;
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(XXHASH_V, full);
        }

        let mut checksum = super::FileChecksum::new(filename.as_str())?;
        assert_eq!(super::super::READ_BUFFER_SIZE as u64, checksum.bytes_read);
        assert_eq!(XXHASH_V, checksum.calc_full()?);
        assert_eq!(
            checksum.bytes_read,
            super::super::READ_BUFFER_SIZE as u64 + meta.len()
        );
        // no read file when twice
        assert_eq!(XXHASH_V, checksum.calc_full()?);
        assert_eq!(
            checksum.bytes_read,
            super::super::READ_BUFFER_SIZE as u64 + meta.len()
        );

        let sha512 = super::super::decode_hex_string("60a11fd3b23811788b38f6055943b17d0ad02c74bd06a5ee850698f1bf7f032048ab8677ee03a5d20c5c4c7af807174b4406274dffb3611740180774d2ad67d0")?;
        assert_eq!(sha512, checksum.calc_secure()?);
        assert_eq!(
            checksum.bytes_read,
            super::super::READ_BUFFER_SIZE as u64 + meta.len() * 2
        );

        // no read file when twice
        assert_eq!(sha512, checksum.calc_secure()?);
        assert_eq!(
            checksum.bytes_read,
            super::super::READ_BUFFER_SIZE as u64 + meta.len() * 2
        );

        Ok(())
    }
}
