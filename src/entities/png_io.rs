use std::io;

use super::super::backend::MediaReader;
use super::tiff_ifd;
use super::{
    CHUNK_CRC_LEN, CHUNK_HEADER_LEN, CHUNK_TYPE_EXIF, CHUNK_TYPE_IEND, MAX_CHUNK_BYTES, MAX_CHUNKS,
    PNG_SIGNATURE, PngExif,
};

pub(crate) fn parse_png_exif(r: &mut dyn MediaReader) -> Option<PngExif> {
    let mut sig = [0u8; 8];
    r.read_exact(&mut sig).ok()?;
    if &sig != PNG_SIGNATURE {
        return None;
    }
    for _ in 0..MAX_CHUNKS {
        let mut hdr = [0u8; CHUNK_HEADER_LEN];
        r.read_exact(&mut hdr).ok()?;
        let len = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        let chunk_type = &hdr[4..8];
        if chunk_type == CHUNK_TYPE_EXIF {
            if len > MAX_CHUNK_BYTES {
                return None;
            }
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload).ok()?;
            return tiff_ifd::parse_tiff(&payload);
        }
        if chunk_type == CHUNK_TYPE_IEND {
            return None;
        }
        let skip = i64::try_from(len + CHUNK_CRC_LEN).unwrap_or(i64::MAX);
        r.seek(io::SeekFrom::Current(skip)).ok()?;
    }
    None
}
