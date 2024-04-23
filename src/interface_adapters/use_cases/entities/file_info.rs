use std::fs;
use std::fs::Metadata;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;

use generic_array::GenericArray;
use memmap2::Mmap;
use sha2::Digest;
use sha2::Sha512;

use super::exif;
use super::SecureHash;

const FAST_READ_SIZE: usize = 4096;
const VALID_DATE_TIME: u64 = 946684800; // 2001-01-01T00:00:00Z

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lazy {
    bytes_read: u64,
    // true if full_hash is the whole file hash
    full: bool,
    // 64 bit hash from the whole file
    hash: u64,
    // Secure hash from the whole file
    secure_hash: SecureHash,
}
impl Lazy {
    // 初始化时，hash是作为第二个快速hash使用的，并不是整个文件的hash
    fn new(bytes_read: u64, hash: u64) -> Self {
        Self {
            bytes_read,
            hash,
            full: false,
            secure_hash: GenericArray::default(),
        }
    }
}
#[derive(Debug)]
pub struct Info {
    // 64 bit hash  from the first FAST_READ_SIZE bytes
    pub fast_hash: u64,
    pub full_path: String,
    pub size: u64,

    meta: Metadata,
    lazy: Mutex<Lazy>,
}
impl Info {
    pub fn from_path(path: &Path) -> io::Result<Self> {
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

        let p = Self::get_full_path(path)?;

        let (bytes_read, first_hash, second_hash) = fast_hash(p.as_str())?;

        Ok(Self {
            fast_hash: first_hash,
            full_path: p,
            size: meta.len(),
            meta,
            lazy: Mutex::new(Lazy::new(bytes_read as u64, second_hash)),
        })
    }

    pub fn bytes_read(&self) -> u64 {
        if let Ok(l) = self.lazy.lock() {
            l.bytes_read
        } else {
            0
        }
    }

    pub fn get_full_path(path: &Path) -> io::Result<String> {
        let p = path.canonicalize()?;
        let p = match p.to_str() {
            Some(s) => s,
            None => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("invalid filename {}", path.display()),
                ));
            }
        };
        let p = p.strip_prefix("\\\\?\\").unwrap_or(p);
        Ok(p.to_string())
    }

    pub fn from(path: &str) -> io::Result<Self> {
        let path = fs::canonicalize(path)?;

        Self::from_path(path.as_path())
    }

    pub fn calc_full_hash(&self) -> io::Result<u64> {
        match self.lazy.lock() {
            Ok(mut l) => {
                if l.full {
                    return Ok(l.hash);
                }

                let (bytes_read, full) = full_hash(self.full_path.as_str())?;

                l.hash = full;
                l.bytes_read += bytes_read as u64;
                l.full = true;
                Ok(full)
            }
            Err(e) => Err(Error::new(io::ErrorKind::Other, e.to_string())),
        }
    }

    fn full_hash(&self) -> u64 {
        if let Ok(l) = self.lazy.try_lock() {
            l.hash
        } else {
            0
        }
    }

    pub fn secure_hash(&self) -> io::Result<SecureHash> {
        match self.lazy.lock() {
            Ok(mut l) => {
                if l.secure_hash != GenericArray::default() {
                    return Ok(l.secure_hash);
                }

                let (bytes_read, secure) = secure_hash(self.full_path.as_str())?;
                l.bytes_read += bytes_read as u64;
                l.secure_hash = secure;
                Ok(secure)
            }
            Err(e) => Err(Error::new(io::ErrorKind::Other, e.to_string())),
        }
    }

    pub fn modified_time(&self) -> io::Result<SystemTime> {
        let exif = exif::Exif::from(self.full_path.as_str()).unwrap_or(None);
        if exif.is_none() {
            return self.meta.modified();
        }
        let exif = exif.unwrap();

        if let Some(mime_type) = exif.mime_type {
            if !(mime_type.starts_with("image") || mime_type.starts_with("video")) {
                return self.meta.modified();
            }
        } else {
            return self.meta.modified();
        }

        let t = if let Some(t) = exif.date_time_original {
            t
        } else if let Some(t) = exif.h264_date_time_original {
            t
        } else if let Some(t) = exif.qt_date_time {
            t
        } else if let Some(t) = exif.exif_create_date {
            t
        } else if let Some(t) = exif.file_modify_date {
            // 	if meta.FileModifyDate > meta.FileCreateDate && meta.FileCreateDate > validDataTime {
            // 		return meta.FileCreateDate
            // 	}
            t
        } else if let Some(t) = exif.file_create_date {
            t
        } else {
            0
        };

        if t > VALID_DATE_TIME {
            Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(t))
        } else {
            self.meta.modified()
        }
    }
}

impl PartialEq for Info {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}

fn fast_hash(path: &str) -> io::Result<(usize, u64, u64)> {
    let mut file = fs::File::open(path)?;

    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file.read(&mut buffer)?;

    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));

    Ok((bytes_read, short, full))
}

fn full_hash(path: &str) -> io::Result<(usize, u64)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    Ok((mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap)))
}

fn secure_hash(path: &str) -> io::Result<(usize, SecureHash)> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok((mmap.len(), Sha512::digest(&mmap)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::io::Read;
    use std::io::Seek;

    use sha2::Digest;
    use wyhash;
    use xxhash_rust::xxh3;

    use super::Info;
    use super::super::test_common as common;

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

    #[test]
    fn small_file() -> common::Result {
        let ct = HashTest::new(common::DATA_SMALL)?;
        assert_eq!(ct.short_wyhash, common::DATA_SMALL_WYHASH);
        assert_eq!(ct.short_xxhash, common::DATA_SMALL_XXHASH);
        assert!(ct.file_size <= super::FAST_READ_SIZE);
        assert_eq!(ct.short_read, ct.file_size);
        assert_eq!(ct.short_xxhash, ct.full);
        assert_eq!(ct.secure, common::data_small_sha512());

        let f = Info::from(common::DATA_SMALL)?;
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash()?, ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash()?, common::data_small_sha512());
        assert_eq!(f.secure_hash()?, common::data_small_sha512());

        Ok(())
    }

    #[test]
    fn large_file() -> common::Result {
        let ct = HashTest::new(common::DATA_LARGE)?;
        assert_eq!(ct.short_wyhash, common::DATA_LARGE_WYHASH);
        assert_ne!(ct.short_xxhash, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.short_read, super::FAST_READ_SIZE);
        assert!(ct.short_read < ct.file_size);
        assert_eq!(ct.full, common::DATA_LARGE_XXHASH);
        assert_eq!(ct.secure, common::data_large_sha512());

        let f = Info::from(common::DATA_LARGE)?;
        assert_eq!(f.fast_hash, ct.short_wyhash);
        assert_eq!(f.full_hash(), ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full_hash()?, ct.full);
        assert_eq!(f.full_hash(), ct.full);
        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(f.secure_hash()?, common::data_large_sha512());

        Ok(())
    }

    #[test]
    fn bytes_read() -> common::Result {
        let meta = fs::metadata(common::DATA_LARGE)?;

        {
            let (bytes_read, _fast, _full) = super::fast_hash(common::DATA_LARGE)?;
            assert_eq!(bytes_read, super::FAST_READ_SIZE);

            let (bytes_read, full) = super::full_hash(common::DATA_LARGE)?;
            assert_eq!(bytes_read as u64, meta.len());
            assert_eq!(full, common::DATA_LARGE_XXHASH);
        }

        let f = super::Info::from(common::DATA_LARGE)?;
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64);
        assert_eq!(f.calc_full_hash()?, common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());
        // no read file when twice
        assert_eq!(f.calc_full_hash()?, common::DATA_LARGE_XXHASH);
        assert_eq!(f.bytes_read(), super::FAST_READ_SIZE as u64 + meta.len());

        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        // no read file when twice
        assert_eq!(f.secure_hash()?, common::data_large_sha512());
        assert_eq!(
            f.bytes_read(),
            super::FAST_READ_SIZE as u64 + meta.len() * 2
        );

        Ok(())
    }

    #[test]
    fn same_small() -> common::Result {
        let f1 = Info::from(common::DATA_SMALL)?;
        let f2 = Info::from(common::DATA_SMALL_COPY)?;

        assert_eq!(f1, f2);
        f1.calc_full_hash()?;

        assert_eq!(f1, f2);
        Ok(())
    }

    #[test]
    fn same_large() -> common::Result {
        let f1 = Info::from(common::DATA_LARGE)?;
        let f2 = Info::from(common::DATA_LARGE_COPY)?;

        assert_eq!(f1, f2);
        f1.calc_full_hash()?;

        assert_ne!(f1, f2);

        f2.calc_full_hash()?;
        assert_eq!(f1, f2);

        Ok(())
    }

    #[test]
    fn strip_prefix() -> common::Result {
        let path = fs::canonicalize(common::DATA_SMALL)?;
        let _path = path.to_str().unwrap();
        // assert_eq!(
        //     path,
        //     "\\\\?\\D:\\user\\prj\\tidymedia\\tests\\data\\data_small"
        // );
        // assert_eq!(
        //     "D:\\user\\prj\\tidymedia\\tests\\data\\data_small",
        //     path.strip_prefix("\\\\?\\").unwrap()
        // );

        Ok(())
    }
}
