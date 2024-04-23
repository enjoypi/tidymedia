use generic_array::typenum;
use generic_array::GenericArray;

pub type SecureHash = GenericArray<u8, typenum::U64>;

mod exif;
pub mod file_index;
pub mod file_info;
#[cfg(test)]
mod test_common;
