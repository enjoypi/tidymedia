#[cfg(test)]
mod tests {
    use tidymedia::interface_adapters::use_cases::entities::*;

    #[test]
    fn same_small() -> tests::Result {
        let mut checksum1 = FileChecksum::new(tests::DATA_SMALL)?;
        let checksum2 = FileChecksum::new(tests::DATA_SMALL_COPY)?;

        assert_eq!(checksum1, checksum2);
        checksum1.calc_full()?;

        assert_eq!(checksum1, checksum2);
        Ok(())
    }

    #[test]
    fn same_large() -> tests::Result {
        let mut checksum1 = FileChecksum::new(tests::DATA_LARGE)?;
        let mut checksum2 = FileChecksum::new(tests::DATA_LARGE_COPY)?;

        assert_eq!(checksum1, checksum2);
        checksum1.calc_full()?;

        assert_ne!(checksum1, checksum2);

        checksum2.calc_full()?;
        assert_eq!(checksum1, checksum2);

        Ok(())
    }
}
