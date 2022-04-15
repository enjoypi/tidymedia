use std::{fs};
use std::io::{Error, ErrorKind, Read, Result};

#[derive(Debug)]
pub struct Media {
    path: String,
    crc32: u32,
    size: u64,
}

impl Media {
    pub fn new(path: &str) -> Result<Media> {
        let attr = fs::metadata(path)?;
        if !attr.is_file() {
            // let isADir = ErrorKind::IsADirectory;
            return Err(Error::from(ErrorKind::Other));
        }

        let path = fs::canonicalize(path)?;

        if let Some(p) = path.to_str()  {
            return Ok(Media {
                path: String::from(p),
                crc32: 0,
                size: attr.len(),
            })
        }
        Err(Error::from(ErrorKind::InvalidInput))
    }

    pub fn crc32(&mut self) -> Result<u32> {
        if self.crc32 > 0 {
            return Ok(self.crc32);
        }
        let mut file = fs::File::open(self.path())?;
        if self.size < 4096 {
            let mut contents: Vec<u8> = vec!(0; self.size as usize);
            let _ = file.read_to_end(&mut contents)?;
            self.crc32 = crc32fast::hash(contents.as_slice());
        } else {
            let mut contents: [u8; 4096] = [0; 4096];
            let _ = file.read(&mut contents)?;
            self.crc32 = crc32fast::hash(&contents);
        }
        Ok(self.crc32)
    }

    pub fn sha1(&mut self) -> Result<Vec<u8> > {
        let mut file = fs::File::open(self.path())?;

        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();

        let mut contents: [u8; 4096] = [0; 4096];
        let _ = file.read(&mut contents)?;
        hasher.update(b"hello world");
        let result= hasher.finalize();
        Ok( result.to_vec())
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }
}