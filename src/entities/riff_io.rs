use std::io;

use super::super::backend::MediaReader;
use super::{
    FOURCC_AVI, FOURCC_HDRL, FOURCC_LIST, FOURCC_RIFF, MAX_HDRL_BYTES, MAX_TOP_CHUNKS, find_strd_in,
};

pub(super) fn find_strd(r: &mut dyn MediaReader) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 12];
    r.read_exact(&mut hdr).ok()?;
    if &hdr[..4] != FOURCC_RIFF || &hdr[8..] != FOURCC_AVI {
        return None;
    }
    for _ in 0..MAX_TOP_CHUNKS {
        let mut ch = [0u8; 8];
        r.read_exact(&mut ch).ok()?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as usize;
        if &ch[..4] == FOURCC_LIST && size >= 4 {
            let mut list_type = [0u8; 4];
            r.read_exact(&mut list_type).ok()?;
            if &list_type == FOURCC_HDRL {
                let body = size - 4;
                if body > MAX_HDRL_BYTES {
                    return None;
                }
                let mut buf = vec![0u8; body];
                r.read_exact(&mut buf).ok()?;
                return find_strd_in(&buf, 0).map(<[u8]>::to_vec);
            }
            skip(r, size - 4 + (size & 1))?;
        } else {
            skip(r, size + (size & 1))?;
        }
    }
    None
}

fn skip(r: &mut dyn MediaReader, n: usize) -> Option<()> {
    let n = i64::try_from(n).unwrap_or(i64::MAX);
    r.seek(io::SeekFrom::Current(n)).ok().map(|_| ())
}
