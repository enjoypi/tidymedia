use std::cell::RefCell;
use std::io;

use sha2::Digest;
use sha2::Sha512;

use crate::entities::SecureHash;
use crate::entities::backend::MediaReader;

pub(super) const FAST_READ_SIZE: usize = 4096;
const STREAM_CHUNK: usize = 1 << 20;

pub fn fast_hash_stream(r: &mut dyn MediaReader) -> io::Result<(usize, u64, u64)> {
    let mut buffer = [0u8; FAST_READ_SIZE];
    let n = read_fill(r, &mut buffer)?;
    let slice = &buffer[..n];
    Ok((
        n,
        wyhash::wyhash(slice, 0),
        xxhash_rust::xxh3::xxh3_64(slice),
    ))
}

thread_local! {
    static CHUNK: RefCell<Vec<u8>> = RefCell::new(vec![0u8; STREAM_CHUNK]);
}

fn drain(r: &mut dyn MediaReader, update: &mut dyn FnMut(&[u8])) -> io::Result<u64> {
    CHUNK.with_borrow_mut(|buf| {
        let mut total = 0u64;
        loop {
            let n = r.read(buf)?;
            if n == 0 {
                return Ok(total);
            }
            update(&buf[..n]);
            total += n as u64;
        }
    })
}

pub fn full_hash_stream(r: &mut dyn MediaReader) -> io::Result<(u64, u64)> {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let total = drain(r, &mut |chunk| hasher.update(chunk))?;
    Ok((total, hasher.digest()))
}

pub fn secure_hash_stream(r: &mut dyn MediaReader) -> io::Result<(u64, SecureHash)> {
    let mut hasher = Sha512::new();
    let total = drain(r, &mut |chunk| hasher.update(chunk))?;
    Ok((total, hasher.finalize()))
}

pub(crate) fn read_fill(r: &mut dyn MediaReader, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}
