#[path = "riff_io.rs"]
mod riff_io;

use super::backend::MediaReader;
use super::tiff_ifd;

const FOURCC_RIFF: &[u8; 4] = b"RIFF";
const FOURCC_AVI: &[u8; 4] = b"AVI ";
const FOURCC_LIST: &[u8; 4] = b"LIST";
const FOURCC_HDRL: &[u8; 4] = b"hdrl";
const FOURCC_STRL: &[u8; 4] = b"strl";
const FOURCC_STRD: &[u8; 4] = b"strd";
const AVIF_MAGIC: &[u8; 4] = b"AVIF";
const IFD_BASE: usize = 8;
const MAX_HDRL_BYTES: usize = 1 << 20;
const MAX_TOP_CHUNKS: usize = 16;
const MAX_LIST_DEPTH: u8 = 4;

pub(crate) type AviExif = tiff_ifd::TiffIfd;

pub(crate) fn parse_avi_exif(r: &mut dyn MediaReader) -> Option<AviExif> {
    parse_avif_ifd(&riff_io::find_strd(r)?)
}

fn find_strd_in(buf: &[u8], depth: u8) -> Option<&[u8]> {
    if depth > MAX_LIST_DEPTH {
        return None;
    }
    let mut off = 0;
    while let Some(size_bytes) = buf.get(off + 4..off + 8) {
        let fourcc = &buf[off..off + 4];
        let size = u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]])
            as usize;
        let body = off + 8;
        if fourcc == FOURCC_STRD {
            return buf.get(body..body + size);
        }
        if fourcc == FOURCC_LIST
            && size >= 4
            && buf.get(body..body + 4).is_some_and(|s| s == FOURCC_STRL)
            && let Some(inner) = buf.get(body + 4..body + size)
            && let Some(hit) = find_strd_in(inner, depth + 1)
        {
            return Some(hit);
        }
        off = body + size + (size & 1);
    }
    None
}

fn parse_avif_ifd(strd: &[u8]) -> Option<AviExif> {
    if strd.get(..4)? != AVIF_MAGIC {
        return None;
    }
    let base = strd.get(IFD_BASE..)?;
    tiff_ifd::parse_ifds(base, 0, tiff_ifd::ByteOrder::Le)
}

#[cfg(test)]
#[path = "riff_fixture_tests.rs"]
mod tests_common;

#[cfg(test)]
#[path = "riff_structure_tests.rs"]
mod structure_tests;

#[cfg(test)]
#[path = "riff_ifd_tests.rs"]
mod ifd_tests;

#[cfg(test)]
#[path = "riff_io_tests.rs"]
mod io_tests;

#[cfg(test)]
#[path = "riff_pump_tests.rs"]
mod pump_tests;
