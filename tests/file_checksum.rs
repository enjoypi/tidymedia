mod common;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Seek};

    use generic_array::GenericArray;
    use xxhash_rust::xxh3;

    use tidymedia::file_checksum::FileChecksum;

    use crate::common;

    #[test]
    fn checksum() -> common::Result {
        const FAST: u64 = 14067286713656012073;
        const FULL: u64 = 0x59d5aae4ebeccc24;
        let sha512 = tidymedia::decode_hex_string("5e43eaa3fc0f18ecdc6e7674dd25d54c31c054489da91dde99c152837258b4637b83aea65dd2f29077df0330b9a3d57a923822399e412d3002ac17e841b2a7be")?;

        let mut f = FileChecksum::new("README.md")?;
        assert_eq!(FAST, f.short);
        assert_eq!(FULL, f.full);
        assert_eq!(GenericArray::default(), f.secure);
        assert_eq!(sha512, f.calc_secure()?);
        assert_eq!(sha512, f.secure);

        let f2 = FileChecksum::new("README.md")?;
        assert_eq!(FAST, f2.short);
        assert_eq!(FULL, f.full);
        assert_eq!(GenericArray::default(), f2.secure);

        assert_eq!(f, f2);

        Ok(())
    }

    const XXHASH_V: u64 = 0xdce253cfb92205e2;

    #[test]
    fn calc_full() -> common::Result {
        let filename = "tests/data0";
        let mut file = fs::File::open(filename)?;

        let (short, full) = {
            let mut buffer = [0; tidymedia::READ_BUFFER_SIZE];
            let bytes_read = file.read(&mut buffer)?;

            (
                wyhash::wyhash(&(buffer[..bytes_read]), 0),
                xxh3::xxh3_64(&(buffer[..bytes_read])),
            )
        };

        let mut buffer = Vec::new();

        file.seek(std::io::SeekFrom::Start(0))?;
        let size = dbg!(file.read_to_end(&mut buffer)?);
        assert!(size > 0);

        assert_eq!(XXHASH_V, xxh3::xxh3_64(buffer.as_slice()));

        let mut f = FileChecksum::new(filename)?;
        assert_eq!(short, f.short);
        assert_eq!(full, f.full);
        assert_eq!(XXHASH_V, f.calc_full()?);

        Ok(())
    }
}
