use generic_array::{typenum, GenericArray};

pub use file_checksum::*;
// pub use file_detail::*;
pub use file_index::*;

mod file_checksum;
mod file_detail;
mod file_index;
pub mod tests;

pub type SecureChecksum = GenericArray<u8, typenum::U64>;
