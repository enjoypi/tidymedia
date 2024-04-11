use std::fs;

use tidymedia::common;

#[test]
fn strip_prefix() -> common::Result {
    let path = fs::canonicalize(common::DATA_SMALL)?;
    let _path = path.to_str().unwrap();
    // assert_eq!(
    //     path,
    //     "\\\\?\\D:\\user\\prj\\tidymedia\\tests\\data\\data_small"
    // );
    // assert_eq!(
    //     "D:\\user\\prj\\tidymedia\\tests\\data\\data_small",
    //     path.strip_prefix("\\\\?\\").unwrap()
    // );

    Ok(())
}
