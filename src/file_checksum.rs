use generic_array::{typenum, GenericArray};
use sha2::{Digest, Sha512};
use std::io::{Error, ErrorKind, Read};
use std::{fs, io};

const READ_BUFFER_SIZE: usize = 4096;

pub type SecureChecksum = GenericArray<u8, typenum::U64>;

#[derive(Debug)]
pub struct FileChecksum {
    pub fast: u64,
    xxhash: u64,
    pub path: String,
    pub secure: SecureChecksum,
    pub size: u64,
}

impl FileChecksum {
    pub fn new(path: &str) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        if !meta.is_file() {
            return Err(Error::from(ErrorKind::Unsupported));
        }

        if meta.len() <= 0 {
            return Err(Error::from(ErrorKind::UnexpectedEof));
        }

        let p = fs::canonicalize(path)?;
        let p = match p.to_str() {
            Some(s) => String::from(s),
            None => return Err(Error::from(ErrorKind::Unsupported)),
        };

        let (wyhash, xxhash) = Self::calc_fast(&p)?;

        Ok(Self {
            fast: wyhash,
            xxhash: xxhash,
            secure: GenericArray::default(),
            path: p,
            size: meta.len(),
        })
    }

    pub fn calc_fast(path: &str) -> io::Result<(u64, u64)> {
        let mut file = fs::File::open(path)?;

        let mut buffer = [0; READ_BUFFER_SIZE];
        let bytes_read = file.read(&mut buffer)?;
        let wyhash = wyhash::wyhash(&(buffer[..bytes_read]), 0);
        let xxhash = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));
        Ok((wyhash, xxhash))
    }

    pub fn calc_secure(&mut self) -> io::Result<SecureChecksum> {
        let mut file = fs::File::open(self.path.as_str())?;
        let mut hasher = Sha512::new();
        let mut buffer: [u8; READ_BUFFER_SIZE] = [0; READ_BUFFER_SIZE];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read > 0 {
                hasher.update(&buffer[..bytes_read]);
            } else {
                break;
            }
        }

        self.secure = hasher.finalize();
        Ok(self.secure)
    }

    pub fn eq(&mut self, other: &mut Self) -> bool {
        if self.size != other.size {
            return false;
        }

        if self.fast != other.fast || self.xxhash != other.xxhash {
            return false;
        }

        if self.secure == GenericArray::default() && self.calc_secure().is_err() {
            return false;
        }

        if other.secure == GenericArray::default() && other.calc_secure().is_err() {
            return false;
        }

        self.secure == other.secure
    }
}

// impl PartialEq for FileChecksum {
//     fn eq(&self, other: &Self) -> bool {
//         self.compare(&mut other.clone())
//     }
// }

// 以下为这个模块的单元测试:
#[cfg(test)]
mod tests {
    use crate::file_checksum::FileChecksum;
    use generic_array::GenericArray;

    #[test]
    fn checksum() {
        const FAST: u64 = 14067286713656012073;
        const CHECKSUM: &str = "5e43eaa3fc0f18ecdc6e7674dd25d54c31c054489da91dde99c152837258b4637b83aea65dd2f29077df0330b9a3d57a923822399e412d3002ac17e841b2a7be";

        let mut f = FileChecksum::new("README.md").unwrap();
        assert_eq!(FAST, f.fast);
        assert_eq!(GenericArray::default(), f.secure);
        let secure = f.calc_secure().unwrap();
        let hex_array: String = secure.iter().map(|byte| format!("{:02x}", byte)).collect();
        assert_eq!(CHECKSUM, hex_array);

        let mut f2 = FileChecksum::new("README.md").unwrap();
        assert_eq!(FAST, f2.fast);
        assert_eq!(GenericArray::default(), f2.secure);
        assert!(f.compare(&mut f2));
        assert_eq!(
            CHECKSUM,
            f2.secure
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect::<String>()
        );
    }
}
