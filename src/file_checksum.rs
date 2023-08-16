use generic_array::{typenum, GenericArray};
use sha2::{Digest, Sha512};
use std::hash::Hasher;
use std::io::{Error, ErrorKind, Read};
use std::{fs, io};

const READ_BUFFER_SIZE: usize = 4096;

pub type SecureChecksum = GenericArray<u8, typenum::U64>;

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
        let meta = fs::metadata(path)?;
        if !meta.is_file() {
            return Err(Error::from(ErrorKind::Unsupported));
        }

        if meta.len() <= 0 {
            return Err(Error::from(ErrorKind::UnexpectedEof));
        }

        let path = fs::canonicalize(path)?;
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
            true_full: meta.len() <= READ_BUFFER_SIZE as u64,
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

    let mut buffer = [0; READ_BUFFER_SIZE];
    let bytes_read = file.read(&mut buffer)?;

    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));

    Ok((bytes_read, short, full))
}

fn full_checksum(path: &str) -> io::Result<(usize, u64)> {
    let mut file = fs::File::open(path)?;
    let mut long_hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buffer: [u8; READ_BUFFER_SIZE] = [0; READ_BUFFER_SIZE];

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

// 以下为这个模块的单元测试:
#[cfg(test)]
mod tests {
    use crate::file_checksum::FileChecksum;
    use generic_array::GenericArray;
    use std::io::{Read, Seek};
    use std::{env, fs};
    use xxhash_rust::xxh3;

    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn checksum() -> Result {
        const FAST: u64 = 14067286713656012073;
        const CHECKSUM: &str = "5e43eaa3fc0f18ecdc6e7674dd25d54c31c054489da91dde99c152837258b4637b83aea65dd2f29077df0330b9a3d57a923822399e412d3002ac17e841b2a7be";

        let mut f = FileChecksum::new("README.md")?;
        assert_eq!(FAST, f.short);
        assert_eq!(GenericArray::default(), f.secure);
        let secure = f.calc_secure()?;
        let hex_array: String = secure.iter().map(|byte| format!("{:02x}", byte)).collect();
        assert_eq!(CHECKSUM, hex_array);

        let mut f2 = FileChecksum::new("README.md")?;
        assert_eq!(FAST, f2.short);
        assert_eq!(GenericArray::default(), f2.secure);
        assert_eq!(f, f2);
        f2.calc_secure()?;
        assert_eq!(
            CHECKSUM,
            f2.secure
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect::<String>()
        );

        Ok(())
    }

    const XXHASH_V: u64 = 0xdce253cfb92205e2;

    #[test]
    fn calc_full() -> Result {
        let home = env::var("HOME")?;

        let filename = home + "/Movies/桥水基金中国区总裁王沿：全天候的投资原则.mp4";
        let mut file = fs::File::open(filename.as_str())?;

        let (short, full) = {
            let mut buffer = [0; super::READ_BUFFER_SIZE];
            let bytes_read = file.read(&mut buffer)?;

            (
                wyhash::wyhash(&(buffer[..bytes_read]), 0),
                xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read])),
            )
        };

        let mut buffer = Vec::new();

        file.seek(std::io::SeekFrom::Start(0))?;
        let size = dbg!(file.read_to_end(&mut buffer)?);
        assert!(size > 0);

        assert_eq!(XXHASH_V, xxh3::xxh3_64(buffer.as_slice()));

        let mut f = FileChecksum::new(filename.as_str())?;
        assert_eq!(short, f.short);
        assert_eq!(full, f.full);
        assert_eq!(XXHASH_V, f.calc_full()?);

        Ok(())
    }

    #[test]
    fn bytes_read() -> Result {
        let home = env::var("HOME")?;
        let filename = home + "/Movies/桥水基金中国区总裁王沿：全天候的投资原则.mp4";
        let meta = fs::metadata(filename.as_str())?;

        {
            let (bytes_read, _fast, _full) = super::fast_checksum(filename.as_str())?;
            assert_eq!(bytes_read, super::READ_BUFFER_SIZE);

            let (bytes_read, full) = super::full_checksum(filename.as_str())?;
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(XXHASH_V, full);
        }

        let mut checksum = FileChecksum::new(filename.as_str())?;
        assert_eq!(super::READ_BUFFER_SIZE as u64, checksum.bytes_read);
        assert_eq!(XXHASH_V, checksum.calc_full()?);
        assert_eq!(
            checksum.bytes_read,
            super::READ_BUFFER_SIZE as u64 + meta.len()
        );
        // no read file when twice
        assert_eq!(XXHASH_V, checksum.calc_full()?);
        assert_eq!(
            checksum.bytes_read,
            super::READ_BUFFER_SIZE as u64 + meta.len()
        );

        Ok(())
    }
}
