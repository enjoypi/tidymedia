use std::io;

use nom_exif::MediaParser;
use nom_exif::MediaSource;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;

pub(super) const SNIFF_BYTES: usize = 256;

pub(super) fn infer_mime(head: &[u8]) -> Option<String> {
    infer::get(head).map(|t| t.mime_type().to_string())
}

pub(super) fn sniff_window(reader: &mut dyn MediaReader) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; SNIFF_BYTES];
    let filled = read_fill(reader, &mut buf)?;
    rewind(reader)?;
    buf.truncate(filled);
    Ok(buf)
}

pub(super) fn read_head(reader: &mut dyn MediaReader, len: usize) -> Vec<u8> {
    let mut head = vec![0u8; len];
    let filled = read_fill(reader, &mut head).unwrap_or(0);
    head.truncate(filled);
    head
}

pub(super) fn rewind(reader: &mut dyn MediaReader) -> io::Result<()> {
    reader.seek(io::SeekFrom::Start(0)).map(|_| ())
}

pub(super) fn parse_image_exif(reader: Box<dyn MediaReader>) -> Option<nom_exif::Exif> {
    let source = MediaSource::seekable(reader).ok()?;
    let mut parser = MediaParser::new();
    parser.parse_exif(source).ok().map(Into::into)
}

pub(super) fn parse_video_track(reader: Box<dyn MediaReader>) -> Option<nom_exif::TrackInfo> {
    let source = MediaSource::seekable(reader).ok()?;
    let mut parser = MediaParser::new();
    parser.parse_track(source).ok()
}
