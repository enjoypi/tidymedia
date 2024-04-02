#[cfg(test)]
mod tests {
    use std::fs;

    use tidymedia::interface_adapters::use_cases::entities::*;

    #[test]
    fn strip_prefix() -> tests::Result {
        let path = fs::canonicalize(tests::DATA_SMALL)?;
        let path = path.to_str().unwrap();
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
}
