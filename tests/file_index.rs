#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tidymedia::interface_adapters::use_cases::entities::tests;
    use tidymedia::interface_adapters::use_cases::entities::*;

    #[test]
    fn insert() -> tests::Result {
        let mut index = FileIndex::new();
        let checksum = index.insert(tests::DATA_SMALL)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(tests::DATA_SMALL)
                .unwrap()
                .to_str()
                .unwrap() // .strip_prefix("\\\\?\\")
                          // .unwrap()
        );
        assert_eq!(checksum.short, tests::DATA_SMALL_WYHASH);
        assert_eq!(checksum.full, tests::DATA_SMALL_XXHASH);

        let mut new_checksum = checksum.clone();
        assert_eq!(new_checksum.calc_secure()?, tests::data_small_sha512());

        Ok(())
    }

    #[test]
    fn search_same() -> tests::Result {
        let mut index = FileIndex::new();
        index.visit_dir(tests::DATA_DIR);

        let same: BTreeMap<u64, _> = index.search_same();
        assert_eq!(same.len(), 2);
        assert_eq!(same[&tests::DATA_LARGE_LEN].len(), 2);

        assert_eq!(
            same[&tests::DATA_LARGE_LEN][0],
            fs::canonicalize(tests::DATA_LARGE)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                   // .unwrap()
        );
        assert_eq!(
            same[&tests::DATA_LARGE_LEN][1],
            fs::canonicalize(tests::DATA_LARGE_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                        // .unwrap()
        );
        assert_eq!(
            same[&tests::DATA_SMALL_LEN][0],
            fs::canonicalize(tests::DATA_SMALL)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                   // .unwrap()
        );
        assert_eq!(
            same[&tests::DATA_SMALL_LEN][1],
            fs::canonicalize(tests::DATA_SMALL_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                        // .unwrap()
        );

        Ok(())
    }
}
