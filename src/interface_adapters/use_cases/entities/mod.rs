pub mod file_checksum;
pub mod file_index;

use generic_array::{typenum, GenericArray};
pub type SecureChecksum = GenericArray<u8, typenum::U64>;
