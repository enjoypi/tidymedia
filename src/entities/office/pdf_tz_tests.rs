use std::io::Cursor;

use chrono::FixedOffset;

use super::{parse_pdf_d_format, parse_tz_offset};
use crate::entities::exif::Exif;

fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

fn east8() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

fn created_of(pdf: &[u8], offset: FixedOffset) -> u64 {
    let exif = Exif::from_reader(
        Box::new(Cursor::new(pdf.to_vec())),
        "application/pdf",
        offset,
    );
    exif.doc_created()
}

fn dated_pdf(date: &str) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n3 0 obj\n<< /CreationDate (".to_vec();
    pdf.extend_from_slice(date.as_bytes());
    pdf.extend_from_slice(b") >>\nendobj\n");
    pdf
}

#[test]
fn pdf_invalid_tz_minute_keeps_wall_clock_at_default_offset() {
    let pdf = dated_pdf("D:20170214103000+08'99'");
    assert_eq!(created_of(&pdf, utc_offset()), 1_487_068_200);
}

#[test]
fn pdf_invalid_tz_minute_uses_config_offset() {
    let pdf = dated_pdf("D:20170214103000+08'99'");
    assert_eq!(created_of(&pdf, east8()), 1_487_039_400);
}

#[test]
fn pdf_out_of_range_total_offset_keeps_wall_clock() {
    let pdf = dated_pdf("D:20170214103000+99'00'");
    assert_eq!(created_of(&pdf, utc_offset()), 1_487_068_200);
}

#[test]
fn pdf_non_digit_tz_keeps_wall_clock() {
    let pdf = dated_pdf("D:20170214103000+XY'00'");
    assert_eq!(created_of(&pdf, utc_offset()), 1_487_068_200);
}

#[test]
fn pdf_invalid_tz_keeps_month_start_bucket() {
    let pdf = dated_pdf("D:20170301000000+08'99'");
    assert_eq!(created_of(&pdf, utc_offset()), 1_488_326_400);
}

#[test]
fn pdf_valid_signed_tz_unchanged() {
    let pdf = dated_pdf("D:20170214103000+08'00'");
    assert_eq!(created_of(&pdf, utc_offset()), 1_487_039_400);
}

#[test]
fn pdf_z_suffix_unchanged() {
    let pdf = dated_pdf("D:20170214103000Z");
    assert_eq!(created_of(&pdf, east8()), 1_487_068_200);
}

#[test]
fn pdf_no_tz_treats_as_utc_unchanged() {
    let pdf = dated_pdf("D:20170214103000");
    assert_eq!(created_of(&pdf, east8()), 1_487_068_200);
}

#[test]
fn pdf_unknown_tz_prefix_treats_as_utc_unchanged() {
    let pdf = dated_pdf("D:20170214103000X");
    assert_eq!(created_of(&pdf, east8()), 1_487_068_200);
}

#[test]
fn pdf_invalid_calendar_still_rejected_with_bad_tz() {
    let pdf = dated_pdf("D:20171314103000+08'99'");
    assert_eq!(created_of(&pdf, utc_offset()), 0);
}

#[test]
fn parse_pdf_d_format_multibyte_in_year_slice_returns_none() {
    assert!(parse_pdf_d_format("D:201中70214103000Z", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_multibyte_in_month_slice_returns_none() {
    assert!(parse_pdf_d_format("D:2017中214103000Z", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_multibyte_in_day_slice_returns_none() {
    assert!(parse_pdf_d_format("D:201702中4103000Z", utc_offset()).is_none());
}

#[test]
fn parse_pdf_d_format_signed_tz_missing_minutes_still_valid() {
    let epoch = parse_pdf_d_format("D:20170214103000+08", utc_offset()).unwrap();
    assert_eq!(epoch, 1_487_039_400);
}

#[test]
fn parse_tz_offset_empty_returns_utc() {
    assert_eq!(parse_tz_offset(b"", east8()).local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_z_returns_utc() {
    assert_eq!(parse_tz_offset(b"Z", east8()).local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_unknown_prefix_returns_utc() {
    assert_eq!(parse_tz_offset(b"X", east8()).local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_positive_hh_mm() {
    assert_eq!(
        parse_tz_offset(b"+08'00'", east8()).local_minus_utc(),
        8 * 3600
    );
}

#[test]
fn parse_tz_offset_negative_hh_mm() {
    assert_eq!(
        parse_tz_offset(b"-05'30'", east8()).local_minus_utc(),
        -(5 * 3600 + 30 * 60)
    );
}

#[test]
fn parse_tz_offset_hh_only_treats_mm_as_zero() {
    assert_eq!(parse_tz_offset(b"+08", east8()).local_minus_utc(), 8 * 3600);
}

#[test]
fn parse_tz_offset_too_short_after_sign_falls_back() {
    assert_eq!(parse_tz_offset(b"+", east8()).local_minus_utc(), 8 * 3600);
}

#[test]
fn parse_tz_offset_non_digit_hh_falls_back() {
    assert_eq!(
        parse_tz_offset(b"+XY'00'", east8()).local_minus_utc(),
        8 * 3600
    );
}

#[test]
fn parse_tz_offset_non_digit_mm_falls_back() {
    assert_eq!(
        parse_tz_offset(b"+08'XY'", east8()).local_minus_utc(),
        8 * 3600
    );
}

#[test]
fn parse_tz_offset_non_utf8_hh_falls_back() {
    assert_eq!(
        parse_tz_offset(b"+\xff\xff", east8()).local_minus_utc(),
        8 * 3600
    );
}

#[test]
fn parse_tz_offset_non_utf8_mm_falls_back() {
    let mut buf: Vec<u8> = b"+08'".to_vec();
    buf.push(0xff);
    buf.push(0xff);
    buf.push(b'\'');
    assert_eq!(parse_tz_offset(&buf, east8()).local_minus_utc(), 8 * 3600);
}

#[test]
fn parse_tz_offset_minute_out_of_range_falls_back() {
    assert_eq!(
        parse_tz_offset(b"+08'99'", east8()).local_minus_utc(),
        8 * 3600
    );
    assert_eq!(
        parse_tz_offset(b"+08'60'", east8()).local_minus_utc(),
        8 * 3600
    );
}

#[test]
fn parse_tz_offset_total_out_of_chrono_range_falls_back() {
    assert_eq!(
        parse_tz_offset(b"+99'00'", east8()).local_minus_utc(),
        8 * 3600
    );
    assert_eq!(
        parse_tz_offset(b"-99'00'", east8()).local_minus_utc(),
        8 * 3600
    );
}
