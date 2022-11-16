use sha2::Digest;
use std::fs;
use std::io::{Error, ErrorKind, Read};

pub type Checksum = [u8; CHECKSUM_SIZE];

pub const CHECKSUM_SIZE: usize = 32;
const READ_BUFFER_SIZE: usize = 4096;

#[derive(Debug)]
pub struct Media {
    path: String,
    size: u64,

    crc32: u32,
    checksum: bool,
    sha256: Checksum,
}

impl Media {
    pub fn new(path: &str) -> std::io::Result<Media> {
        let attr = fs::metadata(path)?;
        if !attr.is_file() {
            // let isADir = ErrorKind::IsADirectory;
            return Err(Error::from(ErrorKind::Other));
        }

        let path = fs::canonicalize(path)?;

        if let Some(p) = path.to_str() {
            return Ok(Media {
                checksum: false,
                crc32: 0,
                sha256: [0; CHECKSUM_SIZE],
                path: String::from(p),
                size: attr.len(),
            });
        }
        Err(Error::from(ErrorKind::InvalidInput))
    }

    pub fn crc32(&self) -> Option<u32> {
        if self.crc32 > 0 {
            return Some(self.crc32);
        }
        None
    }

    pub fn full_crc32(&mut self) -> std::io::Result<u32> {
        let mut file = fs::File::open(self.path())?;
        let mut contents: Vec<u8> = vec![0; self.size as usize];
        let _ = file.read_to_end(&mut contents)?;
        self.crc32 = crc32fast::hash(contents.as_slice());
        Ok(self.crc32)
    }

    pub fn get_crc32(&mut self) -> std::io::Result<u32> {
        if self.crc32 > 0 {
            return Ok(self.crc32);
        }
        let mut file = fs::File::open(self.path())?;
        if self.size < READ_BUFFER_SIZE as u64 {
            let mut contents: Vec<u8> = vec![0; self.size as usize];
            let _ = file.read_to_end(&mut contents)?;
            self.crc32 = crc32fast::hash(contents.as_slice());
        } else {
            let mut contents: [u8; READ_BUFFER_SIZE] = [0; READ_BUFFER_SIZE];
            let _ = file.read(&mut contents)?;
            self.crc32 = crc32fast::hash(&contents);
        }
        Ok(self.crc32)
    }

    pub fn sha256(&self) -> Option<Checksum> {
        if self.checksum {
            return Some(self.sha256);
        }
        None
    }

    pub fn get_sha256(&mut self) -> std::io::Result<Checksum> {
        if self.checksum {
            return Ok(self.sha256);
        }

        let mut file = fs::File::open(self.path())?;
        let mut hasher = sha2::Sha256::new();
        let mut contents: [u8; READ_BUFFER_SIZE] = [0; READ_BUFFER_SIZE];
        loop {
            let size = file.read(&mut contents)?;
            if size > 0 {
                hasher.update(&contents[0..size]);
            } else {
                break;
            }
        }

        let mut result: Checksum = [0; CHECKSUM_SIZE];
        hasher.finalize_into(&mut generic_array::GenericArray::from_mut_slice(
            &mut result,
        ));
        self.checksum = true;
        self.sha256 = result;
        Ok(result)
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }
}
