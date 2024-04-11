use tidymedia::common;
use tidymedia::interface_adapters::use_cases::entities::*;

#[test]
fn same_small() -> common::Result {
    let mut checksum1 = file_checksum::FileChecksum::new(common::DATA_SMALL)?;
    let checksum2 = file_checksum::FileChecksum::new(common::DATA_SMALL_COPY)?;

    assert_eq!(checksum1, checksum2);
    checksum1.calc_full()?;

    assert_eq!(checksum1, checksum2);
    Ok(())
}

#[test]
fn same_large() -> common::Result {
    let mut checksum1 = file_checksum::FileChecksum::new(common::DATA_LARGE)?;
    let mut checksum2 = file_checksum::FileChecksum::new(common::DATA_LARGE_COPY)?;

    assert_eq!(checksum1, checksum2);
    checksum1.calc_full()?;

    assert_ne!(checksum1, checksum2);

    checksum2.calc_full()?;
    assert_eq!(checksum1, checksum2);

    Ok(())
}
