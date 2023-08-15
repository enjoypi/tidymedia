#[cfg(test)]
mod tests {

    #[test]
    fn wyhash() {
        let contents = "# tidymedia\nTidy Media\n";

        use wyhash::wyhash;

        let checksum = wyhash(contents.as_bytes(), 0);
        assert_eq!(14067286713656012073, checksum);

        const XXH64SUM: u64 = 0xda4c8151a1c49e6d;
        const XXH3SUM: u64 = 0x59d5aae4ebeccc24;
        let checksum = twox_hash::xxh3::hash64(contents.as_bytes());
        let checksum2 = xxhash_rust::xxh3::xxh3_64(contents.as_bytes());

        // assert_eq!(XXH64SUM, checksum);
        assert_eq!(XXH3SUM, checksum2);
    }
}
