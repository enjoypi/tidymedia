extern crate core;

use generic_array::{typenum, GenericArray};

pub mod file_checksum;
pub mod file_index;

pub type SecureChecksum = GenericArray<u8, typenum::U64>;

pub type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

pub const READ_BUFFER_SIZE: usize = 4096;

pub fn decode_hex_string(
    input_str: &str,
) -> std::result::Result<SecureChecksum, Box<dyn std::error::Error>> {
    // Step 1: 将16进制字符串转换成 Vec<u8>
    let vec: Vec<u8> = hex::decode(&input_str)?;

    if vec.len() != 64 {
        // 为了适应U64类型，我们需要确保数组里面有64项
        Err("Hex string length doesn't match U64")?;
    }

    // Step 2: 创建 GenericArray
    let generic_array = GenericArray::from_exact_iter(vec).unwrap();

    Ok(generic_array)
}
