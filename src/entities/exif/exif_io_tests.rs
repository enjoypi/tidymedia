use std::io;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use super::super::test_common as common;
use super::MediaReader;
use super::exif_io;

#[derive(Debug)]
struct ErrReader;

impl Read for ErrReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("read refused"))
    }
}

impl Seek for ErrReader {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

#[derive(Debug)]
struct FailSeek(Cursor<Vec<u8>>);

impl Read for FailSeek {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.0.read(b)
    }
}

impl Seek for FailSeek {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::other("seek refused"))
    }
}

#[test]
fn infer_mime_detects_jpeg() {
    let data = std::fs::read(common::DATA_JPEG_WITH_EXIF).unwrap();
    let head = data.get(..64).unwrap_or(&data);
    assert_eq!(exif_io::infer_mime(head).as_deref(), Some("image/jpeg"));
}

#[test]
fn infer_mime_unknown_bytes_returns_none() {
    assert!(exif_io::infer_mime(&[0u8; 256]).is_none());
}

#[test]
fn sniff_window_reads_prefix_and_rewinds() {
    let mut cursor = Cursor::new(vec![7u8; 100]);
    let head = exif_io::sniff_window(&mut cursor).unwrap();
    assert_eq!(head, vec![7u8; 100]);
    assert_eq!(cursor.position(), 0);
}

#[test]
fn sniff_window_read_shorter_than_window_returns_prefix() {
    let mut cursor = Cursor::new(vec![9u8; 16]);
    let head = exif_io::sniff_window(&mut cursor).unwrap();
    assert_eq!(head, vec![9u8; 16]);
}

#[test]
fn sniff_window_propagates_read_error() {
    let mut reader = ErrReader;
    let err = exif_io::sniff_window(&mut reader).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Other);
}

#[test]
fn sniff_window_propagates_seek_error() {
    let mut reader = FailSeek(Cursor::new(vec![1u8; 32]));
    assert!(exif_io::sniff_window(&mut reader).is_err());
}

#[test]
fn read_head_truncates_to_available_bytes() {
    let mut cursor = Cursor::new(vec![3u8; 10]);
    assert_eq!(exif_io::read_head(&mut cursor, 64), vec![3u8; 10]);
}

#[test]
fn read_head_swallows_read_error_to_empty() {
    let mut reader = ErrReader;
    assert_eq!(exif_io::read_head(&mut reader, 64), Vec::<u8>::new());
}

#[test]
fn rewind_resets_position() {
    let mut cursor = Cursor::new(vec![1u8; 8]);
    cursor.set_position(5);
    exif_io::rewind(&mut cursor).unwrap();
    assert_eq!(cursor.position(), 0);
}

#[test]
fn rewind_propagates_seek_error() {
    let mut reader = FailSeek(Cursor::new(vec![1u8; 8]));
    assert!(exif_io::rewind(&mut reader).is_err());
}

#[test]
fn parse_image_exif_reads_jpeg_fixture() {
    let data = std::fs::read(common::DATA_JPEG_WITH_EXIF).unwrap();
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(data));
    let parsed = exif_io::parse_image_exif(reader).expect("fixture jpeg must parse");
    assert!(parsed.get(nom_exif::ExifTag::DateTimeOriginal).is_some());
    assert!(parsed.get(nom_exif::ExifTag::Make).is_none());
}

#[test]
fn parse_image_exif_rejects_non_image_bytes() {
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(vec![0u8; 256]));
    assert!(exif_io::parse_image_exif(reader).is_none());
}

#[test]
fn parse_video_track_reads_mp4_fixture() {
    let data = std::fs::read(common::DATA_MP4_WITH_TRACK).unwrap();
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(data));
    let track = exif_io::parse_video_track(reader).expect("fixture mp4 must parse");
    assert!(track.get(nom_exif::TrackInfoTag::CreateDate).is_some());
}

#[test]
fn parse_video_track_rejects_non_video_bytes() {
    let reader: Box<dyn MediaReader> = Box::new(Cursor::new(vec![0u8; 256]));
    assert!(exif_io::parse_video_track(reader).is_none());
}
