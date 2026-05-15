use std::fs;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use generic_array::GenericArray;
use memmap2::Mmap;
use parking_lot::Mutex;
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

pub struct Info {
    // 64 bit hash  from the first FAST_READ_SIZE bytes
    pub fast_hash: u64,
    pub full_path: Utf8PathBuf,
    pub size: u64,

    // exif info
    exif: Option<exif::Exif>,
    lazy: Mutex<Lazy>,
    meta: fs::Metadata,
}

impl std::fmt::Debug for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fast_hash: {}, size: {}\n{:#?}",
            self.fast_hash, self.size, self.exif
        )
    }
}

impl Info {
    pub fn from(path: &str) -> io::Result<Self> {
        let full_path = full_path(path)?;
        let meta = full_path.metadata()?;
        if !meta.is_file() {
            return Err(Error::new(
                ErrorKind::Other,
                format!("{} is a directory", full_path),
            ));
        }

        if meta.len() == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{} is empty", full_path),
            ));
        }

        let (bytes_read, first_hash, second_hash) = fast_hash(full_path.as_str())?;

        Ok(Self {
            fast_hash: first_hash,
            full_path,
            size: meta.len(),
            exif: None,
            lazy: Mutex::new(Lazy::new(bytes_read as u64, second_hash)),
            meta,
        })
    }

    pub fn from_path(path: &Utf8Path) -> io::Result<Self> {
        Self::from(path.as_str())
    }

    pub fn bytes_read(&self) -> u64 {
        self.lazy.lock().bytes_read
    }

    pub fn calc_full_hash(&self) -> io::Result<u64> {
        let mut l = self.lazy.lock();
        if l.full {
            return Ok(l.hash);
        }

        let (bytes_read, full) = full_hash(self.full_path.as_str())?;

        l.hash = full;
        l.bytes_read += bytes_read as u64;
        l.full = true;
        Ok(full)
    }

    fn full_hash(&self) -> u64 {
        match self.lazy.try_lock() {
            Some(l) => l.hash,
            None => 0,
        }
    }

    pub fn secure_hash(&self) -> io::Result<SecureHash> {
        let mut l = self.lazy.lock();
        if l.secure_hash != GenericArray::default() {
            return Ok(l.secure_hash);
        }

        let (bytes_read, secure) = secure_hash(self.full_path.as_str())?;
        l.bytes_read += bytes_read as u64;
        l.secure_hash = secure;
        Ok(secure)
    }

    #[cfg(test)]
    pub fn exif(&self) -> Option<&exif::Exif> {
        self.exif.as_ref()
    }
    pub fn set_exif(&mut self, exif: exif::Exif) {
        self.exif = Some(exif);
    }

    pub fn create_time(&self) -> io::Result<SystemTime> {
        let file_create_time = self.meta.created()?;
        let file_modify_time = self.meta.modified()?;

        let real_create_time = if file_modify_time < file_create_time {
            file_modify_time
        } else {
            file_create_time
        };

        if self.exif.is_none() {
            return Ok(real_create_time);
        }
        let exif = self.exif.as_ref().unwrap();

        let t = exif.media_create_date();
        if t > VALID_DATE_TIME {
            Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(t))
        } else {
            Ok(real_create_time)
        }
    }

    pub fn is_media(&self) -> bool {
        if self.exif.is_none() {
            return false;
        }
        self.exif.as_ref().unwrap().is_media()
    }
}

impl PartialEq for Info {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}

pub fn full_path(path: &str) -> io::Result<Utf8PathBuf> {
    let full = Utf8Path::new(path);
    if full.is_absolute() {
        return Ok(full.to_path_buf());
    }

    let full = full.canonicalize_utf8()?;
    Ok(Utf8PathBuf::from(strip_windows_unc(full.as_str())))
}

pub(crate) fn strip_windows_unc(path: &str) -> &str {
    if cfg!(target_os = "windows") {
        path.strip_prefix(r"\\?\").unwrap_or(path)
    } else {
        path
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
    fn strip_windows_unc_removes_prefix_only_on_windows() {
        let input = r"\\?\C:\Users\user\prj\tidymedia\tests\data\data_small";
        let got = super::strip_windows_unc(input);
        if cfg!(target_os = "windows") {
            assert_eq!(got, r"C:\Users\user\prj\tidymedia\tests\data\data_small");
        } else {
            assert_eq!(got, input);
        }
    }

    #[test]
    fn strip_windows_unc_passes_through_when_no_prefix() {
        let input = "/home/ecs-user/tidymedia/tests/data/data_small";
        assert_eq!(super::strip_windows_unc(input), input);
    }

    #[test]
    fn full_path_absolute_passthrough() -> common::Result {
        let abs = if cfg!(target_os = "windows") {
            "C:\\windows\\path"
        } else {
            "/tmp"
        };
        let got = super::full_path(abs)?;
        assert_eq!(got.as_str(), abs);
        Ok(())
    }

    #[test]
    fn full_path_relative_canonicalizes() -> common::Result {
        let got = super::full_path(common::DATA_SMALL)?;
        assert!(got.is_absolute(), "expected absolute, got {got}");
        assert!(got.as_str().ends_with("tests/data/data_small")
            || got.as_str().ends_with(r"tests\data\data_small"),
            "unexpected canonical path: {got}");
        Ok(())
    }

    #[test]
    fn full_path_missing_path_errors() {
        let err = super::full_path("definitely-not-a-real-path-xyz123").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
