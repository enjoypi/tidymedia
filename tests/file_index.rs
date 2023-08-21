#[cfg(test)]
mod tests {
    use std::fs;

    use tidymedia::file_index::FileIndex;
    use tidymedia::tests;

    #[test]
    fn insert() -> tests::Result {
        let mut index = FileIndex::new();
        let checksum = index.insert(tests::DATA_SMALL)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(tests::DATA_SMALL)
                .unwrap()
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap()
        );
        assert_eq!(checksum.short, tests::DATA_SMALL_WYHASH);
        assert_eq!(checksum.full, tests::DATA_SMALL_XXHASH);

        let mut new_checksum = checksum.clone();
        assert_eq!(new_checksum.calc_secure()?, tests::data_small_sha512());

        Ok(())
    }
}
