#[path = "png_io.rs"]
mod png_io;

use super::tiff_ifd;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const CHUNK_TYPE_LEN: usize = 4;
const CHUNK_HEADER_LEN: usize = 8;
const CHUNK_CRC_LEN: usize = 4;
const MAX_CHUNK_BYTES: usize = 1 << 20;
const MAX_CHUNKS: usize = 64;
const CHUNK_TYPE_EXIF: &[u8; CHUNK_TYPE_LEN] = b"eXIf";
const CHUNK_TYPE_IEND: &[u8; CHUNK_TYPE_LEN] = b"IEND";

pub(crate) type PngExif = tiff_ifd::TiffIfd;

pub(crate) use self::png_io::parse_png_exif;

#[cfg(test)]
#[path = "png_tests.rs"]
mod tests;
