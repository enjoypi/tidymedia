use generic_array::{typenum, GenericArray};

pub type SecureHash = GenericArray<u8, typenum::U64>;

pub mod file_index;
pub mod file_meta;
#[cfg(test)]
mod test_common;
