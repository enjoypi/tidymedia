use generic_array::{typenum, GenericArray};

pub type SecureChecksum = GenericArray<u8, typenum::U64>;

pub mod file_index;
pub mod file_meta;
#[cfg(test)]
mod test_common;
