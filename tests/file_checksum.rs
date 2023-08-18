mod common;
#[cfg(test)]
mod tests {
    use generic_array::GenericArray;
    use std::io::{Read, Seek};
    use std::{fs, io};

    use sha2::Digest;
    use xxhash_rust::xxh3;

    use tidymedia::file_checksum::FileChecksum;
    use tidymedia::SecureChecksum;

    use crate::common;

    struct ChecksumTest {
        short_wyhash: u64,
        short_xxhash: u64,
        short_read: usize,
        full: u64,
        file_size: usize,

        secure: SecureChecksum,
    }

    impl ChecksumTest {
        fn new(path: &str) -> io::Result<ChecksumTest> {
            let mut file = fs::File::open(path)?;

            let mut buffer = [0; tidymedia::READ_BUFFER_SIZE];
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
    fn small_file() -> common::Result {
        let ct = ChecksumTest::new(common::DATA_SMALL)?;
        assert_eq!(ct.short_wyhash, common::DATA_SMALL_WYHASH);
        assert_eq!(ct.short_xxhash, common::DATA_SMALL_XXHASH);
        assert!(ct.file_size <= tidymedia::READ_BUFFER_SIZE);
        assert_eq!(ct.short_read, ct.file_size);
        assert_eq!(ct.short_xxhash, ct.full);
        assert_eq!(ct.secure, common::data_small_sha512());

        let mut f = FileChecksum::new(common::DATA_SMALL)?;
        assert_eq!(f.short, ct.short_wyhash);
        assert_eq!(f.full, ct.short_xxhash);
        assert_eq!(f.size, ct.file_size as u64);
        assert_eq!(f.calc_full()?, ct.full);
        assert_eq!(f.full, ct.full);
        assert_eq!(f.secure, GenericArray::default());
        assert_eq!(f.calc_secure()?, common::data_small_sha512());
        assert_eq!(f.secure, common::data_small_sha512());

        Ok(())
    }
}
