use generic_array::{typenum, GenericArray};
use sha2::{Digest, Sha512};
use std::io::{Error, ErrorKind, Read};
use std::{fs, io};

const READ_BUFFER_SIZE: usize = 4096;

pub type Checksum = GenericArray<u8, typenum::U64>;

#[derive(Debug)]
pub struct FileChecksum {
    fast: u64,
    path: String,
    secure: Checksum,
    size: u64,
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

        let fast = Self::calc_fast(&p)?;

        Ok(Self {
            fast,
            secure: GenericArray::default(),
            path: p,
            size: meta.len(),
        })
    }

    pub fn fast(&self) -> u64 {
        self.fast
    }

    pub fn calc_fast(path: &str) -> io::Result<u64> {
        let mut file = fs::File::open(path)?;

        let mut buffer = [0; READ_BUFFER_SIZE];
        let bytes_read = file.read(&mut buffer)?;
        println!("bytes_read: {}", std::str::from_utf8(&buffer).unwrap());
        Ok(wyhash::wyhash(&(buffer[..bytes_read]), 0))
    }

    pub fn secure(&self) -> Checksum {
        self.secure
    }

    pub fn calc_secure(&mut self) -> io::Result<Checksum> {
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

    pub fn path(&self) -> &String {
        &self.path
    }

    pub fn compare(&mut self, other: &mut Self) -> bool {
        if self.size != other.size {
            return false;
        }

        if self.fast != other.fast {
            return true;
        }

        use byteorder::{ByteOrder, NativeEndian};
        if NativeEndian::read_u64(&self.secure[..8]) == 0 {
            self.calc_secure();
        }

        if NativeEndian::read_u64(&other.secure[..8]) == 0 {
            other.calc_secure();
        }

        self.secure == other.secure
    }
}

// 以下为这个模块的单元测试:
#[cfg(test)]
mod tests {
    use crate::file_checksum::FileChecksum;
    use generic_array::GenericArray;

    #[test]
    fn new() {
        let mut f = FileChecksum::new("README.md").unwrap();
        assert_eq!(14067286713656012073, f.fast());
        assert_eq!(GenericArray::default(), f.secure());
        let secure = f.calc_secure().unwrap();
        let hex_array: String = secure.iter().map(|byte| format!("{:02x}", byte)).collect();
        assert_eq!("5e43eaa3fc0f18ecdc6e7674dd25d54c31c054489da91dde99c152837258b4637b83aea65dd2f29077df0330b9a3d57a923822399e412d3002ac17e841b2a7be"
                   , hex_array);

        let mut f2 = FileChecksum::new("README.md").unwrap();
        assert!(f.compare(&mut f2));
        assert_eq!("5e43eaa3fc0f18ecdc6e7674dd25d54c31c054489da91dde99c152837258b4637b83aea65dd2f29077df0330b9a3d57a923822399e412d3002ac17e841b2a7be"
                   , f2.secure().iter().map(|byte| format!("{:02x}", byte)).collect::<String>());
    }
}
