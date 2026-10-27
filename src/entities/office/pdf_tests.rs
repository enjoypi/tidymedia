use std::io::Cursor;

use super::*;
use crate::entities::office::tests_common::utc_offset;

#[test]
fn extract_text_read_error_returns_empty() {
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; 32], 0, 0);
    assert_eq!(extract_text(&mut io, "application/pdf", 64), "");
}

#[test]
fn parse_pdf_d_format_full_with_positive_tz() {
    let epoch = parse_pdf_d_format("D:20170214103000+08'00'", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_039_400);
}

#[test]
fn parse_pdf_d_format_full_with_negative_tz() {
    let epoch = parse_pdf_d_format("D:20170214103000-05'00'", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_086_200);
}

#[test]
fn parse_pdf_d_format_full_with_z_suffix() {
    let epoch = parse_pdf_d_format("D:20170214103000Z", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_068_200);
}

#[test]
fn parse_pdf_d_format_no_tz_treats_as_utc() {
    let epoch = parse_pdf_d_format("D:20170214103000", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_068_200);
}

#[test]
fn parse_pdf_d_format_date_only_minimum_length() {
    let epoch = parse_pdf_d_format("D:20170214", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_030_400);
}

#[test]
fn parse_pdf_d_format_hour_only_truncates_mm_ss() {
    let epoch = parse_pdf_d_format("D:2017021410", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_066_400);
}

#[test]
fn parse_pdf_d_format_missing_d_prefix_returns_none() {
    assert!(parse_pdf_d_format("20170214103000", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_too_short_returns_none() {
    assert!(parse_pdf_d_format("D:2017", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_invalid_year_returns_none() {
    assert!(parse_pdf_d_format("D:XXXX0214", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_invalid_month_returns_none() {
    assert!(parse_pdf_d_format("D:2017XX14", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_invalid_day_returns_none() {
    assert!(parse_pdf_d_format("D:201702XX", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_month_out_of_range_returns_none() {
    assert!(parse_pdf_d_format("D:20171314", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_day_out_of_range_returns_none() {
    assert!(parse_pdf_d_format("D:20170232", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_hour_out_of_range_returns_none() {
    assert!(parse_pdf_d_format("D:2017021425", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_invalid_hour_chars_falls_back_to_zero() {
    let epoch = parse_pdf_d_format("D:20170214XX", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_030_400);
}

#[test]
fn parse_pdf_d_format_pre_epoch_year_returns_none() {
    assert!(parse_pdf_d_format("D:19690101000000Z", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_year_9999_valid() {
    let epoch = parse_pdf_d_format("D:99990101000000Z", utc_offset()).unwrap();
    assert!(epoch > 1_700_000_000);
}

#[test]
fn parse_pair_non_utf8_bytes_returns_none() {
    let bytes = [b'D', b':', 0xff, 0xff];
    assert!(parse_pair(&bytes, 2).is_none());
}

#[test]
fn parse_pair_offset_past_end_returns_none() {
    assert!(parse_pair(b"ab", 5).is_none());
}

#[test]
fn parse_pair_two_digit_ok() {
    assert_eq!(parse_pair(b"12", 0), Some(12));
}

#[test]
fn find_subslice_found() {
    assert_eq!(find_subslice(b"abcdef", b"cd"), Some(2));
}

#[test]
fn find_subslice_not_found() {
    assert!(find_subslice(b"abcdef", b"xyz").is_none());
}

#[test]
fn scan_d_date_after_key_happy_path() {
    let buf = b"/CreationDate (D:20170214103000+08'00') ...";
    assert_eq!(
        scan_d_date_after_key(buf, b"/CreationDate", utc_offset()),
        Some(1_487_039_400)
    );
}

#[test]
fn scan_d_date_after_key_missing_key_returns_none() {
    let buf = b"no key here at all";
    assert!(scan_d_date_after_key(buf, b"/CreationDate", utc_offset()).is_none());
}

#[test]
fn scan_d_date_after_key_missing_open_paren_returns_none() {
    let buf = b"/CreationDate something_no_paren_after";
    assert!(scan_d_date_after_key(buf, b"/CreationDate", utc_offset()).is_none());
}

#[test]
fn scan_d_date_after_key_missing_close_paren_returns_none() {
    let buf = b"/CreationDate (D:20170214 no close paren ever";
    assert!(scan_d_date_after_key(buf, b"/CreationDate", utc_offset()).is_none());
}

#[test]
fn scan_d_date_after_key_invalid_utf8_payload_returns_none() {
    let mut buf: Vec<u8> = b"/CreationDate (".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b")");
    assert!(scan_d_date_after_key(&buf, b"/CreationDate", utc_offset()).is_none());
}

#[test]
fn scan_d_date_after_key_invalid_date_format_returns_none() {
    let buf = b"/CreationDate (not a date)";
    assert!(scan_d_date_after_key(buf, b"/CreationDate", utc_offset()).is_none());
}

#[test]
fn parse_extracts_creation_and_mod_dates() {
    let pdf = b"%PDF-1.4\n3 0 obj\n<< /CreationDate (D:20170214103000+08'00') /ModDate (D:20180101120000Z) >>\nendobj\n";
    let mut r = Cursor::new(pdf.to_vec());
    let (c, m) = parse(&mut r, "application/pdf", utc_offset());
    assert_eq!(c, 1_487_039_400);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn parse_returns_zeros_when_no_info_dict() {
    let pdf = b"%PDF-1.4\nempty document with no /CreationDate";
    let mut r = Cursor::new(pdf.to_vec());
    assert_eq!(parse(&mut r, "application/pdf", utc_offset()), (0, 0));
}

#[test]
fn parse_with_creation_only_returns_zero_for_modified() {
    let pdf = b"%PDF-1.4\n3 0 obj << /CreationDate (D:20170214103000Z) >> endobj";
    let mut r = Cursor::new(pdf.to_vec());
    let (c, m) = parse(&mut r, "application/pdf", utc_offset());
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn parse_drops_middle_region_of_large_pdf() {
    let head_tail = usize::try_from(PDF_SCAN_BYTES).unwrap();
    let mut data = vec![0u8; head_tail + 512];
    data.extend_from_slice(b"/CreationDate (D:20170214103000Z)");
    data.extend(vec![0u8; head_tail + 512]);
    let mut r = Cursor::new(data);
    assert_eq!(parse(&mut r, "application/pdf", utc_offset()), (0, 0));
}

#[test]
fn parse_finds_creation_date_in_tail_window() {
    let filler = usize::try_from(3 * PDF_SCAN_BYTES).unwrap();
    let mut data = vec![b' '; filler];
    data.extend_from_slice(b"trailer << /Info 1 0 R >> /CreationDate (D:20170214103000Z)");
    let mut r = Cursor::new(data);
    let (c, _) = parse(&mut r, "application/pdf", utc_offset());
    assert_eq!(c, 1_487_068_200);
}

#[test]
fn parse_head_window_wins_over_tail() {
    let filler = usize::try_from(3 * PDF_SCAN_BYTES).unwrap();
    let mut data = b"%PDF-1.4 /CreationDate (D:20170214103000Z) ".to_vec();
    data.extend(vec![b' '; filler]);
    data.extend_from_slice(b"/CreationDate (D:20200101000000Z)");
    let mut r = Cursor::new(data);
    let (c, _) = parse(&mut r, "application/pdf", utc_offset());
    assert_eq!(c, 1_487_068_200);
}

#[test]
fn parse_whole_file_read_when_at_most_two_windows() {
    let head = usize::try_from(PDF_SCAN_BYTES).unwrap();
    let mut data = vec![0u8; head + 100];
    data.extend_from_slice(b"/CreationDate (D:20170214103000Z)");
    let mut r = Cursor::new(data);
    let (c, _) = parse(&mut r, "application/pdf", utc_offset());
    assert_eq!(c, 1_487_068_200);
}

#[test]
fn scan_windows_zero_size_single_empty_range() {
    assert_eq!(scan_windows(0), (0..0, None));
}

#[test]
fn scan_windows_small_file_single_window() {
    assert_eq!(scan_windows(100), (0..100, None));
}

#[test]
fn scan_windows_exactly_two_windows_still_whole_read() {
    let two = 2 * PDF_SCAN_BYTES;
    assert_eq!(scan_windows(two), (0..two, None));
}

#[test]
fn scan_windows_above_threshold_splits_disjoint_head_tail() {
    let size = 2 * PDF_SCAN_BYTES + 1;
    let (head, tail) = scan_windows(size);
    let tail = tail.unwrap();
    assert_eq!(head, 0..PDF_SCAN_BYTES);
    assert_eq!(tail, size - PDF_SCAN_BYTES..size);
    assert!(head.end <= tail.start, "windows must not overlap");
}

#[test]
fn scan_windows_large_file_windows_sized_exactly() {
    let size = 100 * PDF_SCAN_BYTES;
    let (head, tail) = scan_windows(size);
    let tail = tail.unwrap();
    assert_eq!(head.end - head.start, PDF_SCAN_BYTES);
    assert_eq!(tail.end - tail.start, PDF_SCAN_BYTES);
}

#[test]
fn parse_returns_zeros_on_read_error() {
    use std::io;

    #[derive(Debug)]
    struct FailRead;
    impl io::Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected"))
        }
    }
    impl io::Seek for FailRead {
        fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
            Ok(1024)
        }
    }

    let mut r = FailRead;
    assert_eq!(parse(&mut r, "application/pdf", utc_offset()), (0, 0));
}

fn windows_of(io: &mut crate::entities::test_common::FlakyIo) -> Option<Vec<u8>> {
    read_windows(io)
}

#[test]
fn read_windows_none_when_seek_end_fails() {
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; 16], 0, u64::MAX);
    assert!(windows_of(&mut io).is_none());
}

#[test]
fn read_windows_none_when_seek_head_fails() {
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; 16], 1, u64::MAX);
    assert!(windows_of(&mut io).is_none());
}

#[test]
fn read_windows_none_when_seek_tail_fails() {
    let size = usize::try_from(2 * PDF_SCAN_BYTES + 1).unwrap();
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; size], 2, u64::MAX);
    assert!(windows_of(&mut io).is_none());
}

#[test]
fn read_windows_none_when_tail_read_fails() {
    let size = usize::try_from(2 * PDF_SCAN_BYTES + 1).unwrap();
    let mut io = crate::entities::test_common::FlakyIo::new(vec![0; size], 3, PDF_SCAN_BYTES);
    assert!(windows_of(&mut io).is_none());
}

#[test]
fn read_windows_reads_head_and_tail() {
    let size = usize::try_from(2 * PDF_SCAN_BYTES + 1).unwrap();
    let mut io = crate::entities::test_common::FlakyIo::new(vec![7; size], 3, u64::MAX);
    let buf = windows_of(&mut io).unwrap();
    assert_eq!(buf.len() as u64, 2 * PDF_SCAN_BYTES);
}
