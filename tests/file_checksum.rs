mod common;
#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Seek};

    use sha2::Digest;
    use xxhash_rust::xxh3;

    use tidymedia::file_checksum::FileChecksum;

    use crate::common;

    #[test]
    fn checksum() -> common::Result {
        let mut file = fs::File::open(common::DATA0)?;

        let (short, first_full) = {
            let mut buffer = [0; tidymedia::READ_BUFFER_SIZE];
            let bytes_read = file.read(&mut buffer)?;
            assert!(bytes_read <= tidymedia::READ_BUFFER_SIZE);

            (
                wyhash::wyhash(&(buffer[..bytes_read]), 0),
                xxh3::xxh3_64(&(buffer[..bytes_read])),
            )
        };

        assert_eq!(common::DATA0_WYHASH, short);
        assert_eq!(common::DATA0_XXHASH, first_full);

        let mut buffer = Vec::new();

        file.seek(std::io::SeekFrom::Start(0))?;
        let size = file.read_to_end(&mut buffer)?;
        assert!(size > 0);

        let second_full = xxh3::xxh3_64(buffer.as_slice());
        assert_eq!(first_full, second_full);

        let mut hasher = sha2::Sha512::new();
        hasher.update(buffer.as_slice());
        let secure = hasher.finalize();
        assert_eq!(secure, common::data0_sha512());

        let mut f = FileChecksum::new(common::DATA0)?;
        assert_eq!(short, f.short);
        assert_eq!(first_full, f.full);
        assert_eq!(second_full, f.calc_full()?);

        Ok(())
    }
}
