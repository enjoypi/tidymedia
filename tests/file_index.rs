#[cfg(test)]
mod tests {
    use std::fs;

    use tidymedia::file_index::FileIndex;

    const DATA0_SHA256: &str= "c77d955d24f36057a2fc6eba10d9a386ef6b8a6568e73bb8f6a168b4e2adc65fa2ffdc6f6e479f42199b740b8e83af74caffa6f580d4b7351be20efa65b0fcd2";
    const FILENAME: &str = "tests/data0";

    #[test]
    fn insert() -> tidymedia::TestResult {
        let mut index = FileIndex::new();
        let checksum = index.insert(FILENAME)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(FILENAME)
                .unwrap()
                .as_path()
                .to_str()
                .unwrap()
        );
        const FAST: u64 = 14067286713656012073;
        assert_eq!(checksum.short, FAST);
        assert_eq!(checksum.full, 0x59d5aae4ebeccc24);

        let checksum = index.files.get_mut(FILENAME).unwrap();
        assert_eq!(
            checksum.calc_secure()?,
            tidymedia::decode_hex_string(DATA0_SHA256)?
        );

        Ok(())
    }
}
