use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use chrono::FixedOffset;

pub(crate) fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).expect("internal: zero offset is within range")
}

#[derive(Debug)]
pub(crate) struct FailReader;

impl Read for FailReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("injected read failure"))
    }
}

impl Seek for FailReader {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

pub(crate) fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }
    cursor.into_inner()
}

pub(crate) fn corrupt_zip_entry(zip: &mut [u8], needle: &[u8]) {
    let pos = zip
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("needle present in zip data");
    zip[pos] ^= 1;
}
