use generic_array::{GenericArray, typenum};

pub type SecureChecksum = GenericArray<u8, typenum::U64>;

pub mod file_checksum;
pub mod file_index;
#[cfg(test)]
mod test_common;
