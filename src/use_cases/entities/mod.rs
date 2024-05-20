use generic_array::GenericArray;
use generic_array::typenum;

pub type SecureHash = GenericArray<u8, typenum::U64>;

mod exif;
pub mod file_index;
pub mod file_info;
#[cfg(test)]
mod test_common;
