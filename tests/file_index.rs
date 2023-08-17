mod common;

#[cfg(test)]
mod tests {
    use std::fs;

    use tidymedia::file_index::FileIndex;

    use crate::common;

    #[test]
    fn insert() -> common::Result {
        let mut index = FileIndex::new();
        let checksum = index.insert(common::DATA0)?;
        assert_eq!(
            checksum.path,
            fs::canonicalize(common::DATA0)
                .unwrap()
                .as_path()
                .to_str()
                .unwrap()
        );
        assert_eq!(checksum.short, common::DATA0_WYHASH);
        assert_eq!(checksum.full, common::DATA0_XXHASH);

        let mut new_checksum = checksum.clone();
        assert_eq!(new_checksum.calc_secure()?, common::data0_sha512());

        Ok(())
    }
}
