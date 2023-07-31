

#[cfg(test)]
mod tests {

    #[test]
    fn one_result() {
        let contents = "abcdefghijkasdfasdfasdfsdf";

        use crc32fast::Hasher;

        let mut hasher = Hasher::new();
        hasher.update(contents.as_bytes());
        let checksum = hasher.finalize();
        assert_eq!(0xCCF8A3B6, checksum);
    }

    #[test]
    fn xxhash() {
        let contents = "abcdefghijkasdfasdfasdfsdf";

        use xxhash_rust::xxh3::xxh3_64;

        let checksum = xxh3_64(contents.as_bytes());
        assert_eq!(10919252494161421102, checksum);
    }

    #[test]
    fn test_wyhash() {
        let contents = "abcdefghijkasdfasdfasdfsdf";

        use wyhash::wyhash;

        let checksum = wyhash(contents.as_bytes(), 0);
        assert_eq!(2804366926580453156, checksum);
    }
}

