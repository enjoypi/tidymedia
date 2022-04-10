use std::{fs};
use std::path::{PathBuf};
use std::io::{Error, ErrorKind, Read, Result};

#[derive(Debug)]
pub struct Multimedia {
    path: PathBuf,
    crc32: u32,
    size: u64,
}

impl Multimedia {
    pub fn new(path: &str) -> Result<Multimedia> {
        let attr = fs::metadata(path)?;
        if !attr.is_file() {
            // let isADir = ErrorKind::IsADirectory;
            return Err(Error::from(ErrorKind::Other));
        }


        Ok(Multimedia {
            path: fs::canonicalize(path)?,
            crc32: 0,
            size: attr.len(),
        })
    }

    pub fn crc32(&mut self) -> Result<u32> {
        if self.crc32 > 0 {
            return Ok(self.crc32);
        }
        let mut file = fs::File::open(self.path.to_str().unwrap())?;
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

    pub fn path(&self) -> &str {
        self.path.to_str().unwrap()
    }
}