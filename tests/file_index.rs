use std::collections::BTreeMap;
use std::fs;

use tidymedia::common;
use tidymedia::interface_adapters::use_cases::entities::*;

#[test]
fn insert() -> common::Result {
    let mut index = file_index::FileIndex::new();
    let checksum = index.insert(common::DATA_SMALL)?;
    assert_eq!(
        checksum.path,
        fs::canonicalize(common::DATA_SMALL)
            .unwrap()
            .to_str()
            .unwrap() // .strip_prefix("\\\\?\\")
                      // .unwrap()
    );
    assert_eq!(checksum.short, common::DATA_SMALL_WYHASH);
    assert_eq!(checksum.full, common::DATA_SMALL_XXHASH);

    let mut new_checksum = checksum.clone();
    assert_eq!(new_checksum.calc_secure()?, common::data_small_sha512());

    Ok(())
}

#[test]
fn search_same() -> common::Result {
    let mut index = file_index::FileIndex::new();
    index.visit_dir(common::DATA_DIR);

    let same: BTreeMap<u64, _> = index.search_same();
    assert_eq!(same.len(), 2);
    assert_eq!(same[&common::DATA_LARGE_LEN].len(), 2);

    assert_eq!(
        same[&common::DATA_LARGE_LEN][0],
        fs::canonicalize(common::DATA_LARGE)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                // .unwrap()
    );
    assert_eq!(
        same[&common::DATA_LARGE_LEN][1],
        fs::canonicalize(common::DATA_LARGE_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                     // .unwrap()
    );
    assert_eq!(
        same[&common::DATA_SMALL_LEN][0],
        fs::canonicalize(common::DATA_SMALL)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                // .unwrap()
    );
    assert_eq!(
        same[&common::DATA_SMALL_LEN][1],
        fs::canonicalize(common::DATA_SMALL_COPY)?.to_str().unwrap() // .strip_prefix("\\\\?\\")
                                                                     // .unwrap()
    );

    Ok(())
}
