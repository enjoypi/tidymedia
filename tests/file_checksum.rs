#[cfg(test)]
mod tests {
    use tidymedia::{file_checksum, tests};

    #[test]
    fn same_small() -> tests::Result {
        let mut checksum1 = file_checksum::FileChecksum::new(tests::DATA_SMALL)?;
        let checksum2 = file_checksum::FileChecksum::new(tests::DATA_SMALL_COPY)?;

        assert_eq!(checksum1, checksum2);
        checksum1.calc_full()?;

        assert_eq!(checksum1, checksum2);
        Ok(())
    }

    #[test]
    fn same_large() -> tests::Result {
        let mut checksum1 = file_checksum::FileChecksum::new(tests::DATA_LARGE)?;
        let mut checksum2 = file_checksum::FileChecksum::new(tests::DATA_LARGE_COPY)?;

        assert_eq!(checksum1, checksum2);
        checksum1.calc_full()?;

        assert_ne!(checksum1, checksum2);

        checksum2.calc_full()?;
        assert_eq!(checksum1, checksum2);

        Ok(())
    }
}
