use std::io::Read;

const INFLATE_OUTPUT_CAP: usize = 2 * 1024 * 1024;

pub(super) fn inflate_capped(data: &[u8]) -> Option<Vec<u8>> {
    let decoder = flate2::read::ZlibDecoder::new(data);
    let mut inflated = Vec::new();
    match decoder
        .take(INFLATE_OUTPUT_CAP as u64)
        .read_to_end(&mut inflated)
    {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(inflated),
    }
}

#[cfg(test)]
#[path = "flate_io_tests.rs"]
mod tests;
