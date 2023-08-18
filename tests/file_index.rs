mod common;

#[cfg(test)]
mod tests {
    use std::fs;

    use tidymedia::file_index::FileIndex;

    use crate::common;

    #[test]
    fn insert() -> common::Result {
        let mut index = FileIndex::new();
        let checksum = index.insert(common::DATA_SMALL)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(common::DATA_SMALL)
                .unwrap()
                .as_path()
                .to_str()
                .unwrap()
        );
        assert_eq!(checksum.short, common::DATA_SMALL_WYHASH);
        assert_eq!(checksum.full, common::DATA_SMALL_XXHASH);

        let mut new_checksum = checksum.clone();
        assert_eq!(new_checksum.calc_secure()?, common::data_small_sha512());

        Ok(())
    }
}
