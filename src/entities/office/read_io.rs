use std::io::{Read, SeekFrom};
use std::ops::Range;

use crate::entities::backend::MediaReader;

pub(super) fn read_capped(reader: &mut dyn MediaReader, cap: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut limited = reader.take(cap as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return None;
    }
    Some(buf)
}

pub(super) fn seek_len(reader: &mut dyn MediaReader) -> Option<u64> {
    reader.seek(SeekFrom::End(0)).ok()
}

pub(super) fn read_range(reader: &mut dyn MediaReader, range: Range<u64>) -> Option<Vec<u8>> {
    reader.seek(SeekFrom::Start(range.start)).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(range.end - range.start).unwrap_or(0));
    reader
        .take(range.end - range.start)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}
