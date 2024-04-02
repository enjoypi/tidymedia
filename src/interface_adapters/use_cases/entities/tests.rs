use super::SecureChecksum;

pub type Error = Box<dyn std::error::Error>;
pub type Result = std::result::Result<(), Error>;

pub fn str_to_secure(input_str: &str) -> SecureChecksum {
    // Step 1: 将16进制字符串转换成 Vec<u8>
    let vec: Vec<u8> = hex::decode(input_str).unwrap();

    if vec.len() != 64 {
        // 为了适应U64类型，我们需要确保数组里面有64项
        generic_array::GenericArray::default()
    } else {
        SecureChecksum::from_exact_iter(vec).unwrap()
    }
}

pub const DATA_DIR: &str = "tests/data";
pub const DATA_SMALL: &str = "tests/data/data_small";
pub const DATA_SMALL_WYHASH: u64 = 13333046383594682858;
pub const DATA_SMALL_XXHASH: u64 = 0x1a5efdfdbd01a44c;
pub fn data_small_sha512() -> SecureChecksum {
    str_to_secure("c77d955d24f36057a2fc6eba10d9a386ef6b8a6568e73bb8f6a168b4e2adc65fa2ffdc6f6e479f42199b740b8e83af74caffa6f580d4b7351be20efa65b0fcd2")
}
pub const DATA_SMALL_COPY: &str = "tests/data/data_small_copy";

pub const DATA_LARGE: &str = "tests/data/data_large";
pub const DATA_LARGE_WYHASH: u64 = 2034553491748707037;
pub const DATA_LARGE_XXHASH: u64 = 0x9dba53c59ea968e9;
pub fn data_large_sha512() -> SecureChecksum {
    str_to_secure("0f7fd3e44b860c33de83c19edb759edcad9c6e101910f765e86e2443f533f9c254ad544a84e4bb56b221620148c79b2b8619cfd8f611d30617c6c32f210bcea7")
}
pub const DATA_LARGE_COPY: &str = "tests/data/data_large_copy";
