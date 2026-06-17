//! PDF 字节扫描单测：helper 微测覆盖 `D:` 格式各分支 + 路由 `parse()` e2e
//! 用手写 PDF 字串（不依赖 fixture 文件 —— PDF 文档化签名 `%PDF-1.4` 起头 +
//! 任意位置的 `/CreationDate (D:...)` 即可被扫描器命中）。

use std::io::Cursor;

use super::*;

// ============= parse_pdf_d_format helper 微测 =============

#[test]
fn parse_pdf_d_format_full_with_positive_tz() {
    // 2017-02-14 10:30:00 +08:00 → UTC 02:30:00 → 1_487_039_400
    let epoch = parse_pdf_d_format("D:20170214103000+08'00'").unwrap();
    assert_eq!(epoch, 1_487_039_400);
}

#[test]
fn parse_pdf_d_format_full_with_negative_tz() {
    // 2017-02-14 10:30:00 -05:00 → UTC 15:30:00
    let epoch = parse_pdf_d_format("D:20170214103000-05'00'").unwrap();
    assert_eq!(epoch, 1_487_086_200);
}

#[test]
fn parse_pdf_d_format_full_with_z_suffix() {
    // 2017-02-14 10:30:00Z（UTC）
    let epoch = parse_pdf_d_format("D:20170214103000Z").unwrap();
    assert_eq!(epoch, 1_487_068_200);
}

#[test]
fn parse_pdf_d_format_no_tz_treats_as_utc() {
    let epoch = parse_pdf_d_format("D:20170214103000").unwrap();
    assert_eq!(epoch, 1_487_068_200);
}

#[test]
fn parse_pdf_d_format_date_only_minimum_length() {
    // D:YYYYMMDD（8 字符），时分秒缺省 → 0
    let epoch = parse_pdf_d_format("D:20170214").unwrap();
    // 2017-02-14 00:00:00 UTC = 1_487_030_400
    assert_eq!(epoch, 1_487_030_400);
}

#[test]
fn parse_pdf_d_format_hour_only_truncates_mm_ss() {
    let epoch = parse_pdf_d_format("D:2017021410").unwrap();
    // 2017-02-14 10:00:00 UTC
    assert_eq!(epoch, 1_487_066_400);
}

#[test]
fn parse_pdf_d_format_missing_d_prefix_returns_none() {
    assert!(parse_pdf_d_format("20170214103000").is_none());
}

#[test]
fn parse_pdf_d_format_too_short_returns_none() {
    assert!(parse_pdf_d_format("D:2017").is_none());
}

#[test]
fn parse_pdf_d_format_invalid_year_returns_none() {
    assert!(parse_pdf_d_format("D:XXXX0214").is_none());
}

#[test]
fn parse_pdf_d_format_invalid_month_returns_none() {
    assert!(parse_pdf_d_format("D:2017XX14").is_none());
}

#[test]
fn parse_pdf_d_format_invalid_day_returns_none() {
    assert!(parse_pdf_d_format("D:201702XX").is_none());
}

#[test]
fn parse_pdf_d_format_month_out_of_range_returns_none() {
    // 第 13 月 → NaiveDate::from_ymd_opt 返 None
    assert!(parse_pdf_d_format("D:20171314").is_none());
}

#[test]
fn parse_pdf_d_format_day_out_of_range_returns_none() {
    assert!(parse_pdf_d_format("D:20170232").is_none());
}

#[test]
fn parse_pdf_d_format_hour_out_of_range_returns_none() {
    // 25:00 → from_hms_opt 返 None
    assert!(parse_pdf_d_format("D:2017021425").is_none());
}

#[test]
fn parse_pdf_d_format_invalid_hour_chars_falls_back_to_zero() {
    // 时分秒解析失败按 0 处理（parse_pair 返 None → unwrap_or(0)）
    // hour 字段是 "XX" → parse_pair None → 0；解析成功为 2017-02-14 00:00:00 UTC
    let epoch = parse_pdf_d_format("D:20170214XX").unwrap();
    assert_eq!(epoch, 1_487_030_400);
}

#[test]
fn parse_pdf_d_format_pre_epoch_year_returns_none() {
    // 1969 年 → epoch 为负 → None
    assert!(parse_pdf_d_format("D:19690101000000Z").is_none());
}

#[test]
fn parse_pdf_d_format_year_9999_valid() {
    // 边界年 9999 仍合法
    let epoch = parse_pdf_d_format("D:99990101000000Z").unwrap();
    assert!(epoch > 1_700_000_000);
}

/// 覆盖 `parse_tz_offset(&bytes[14..])?` Err arm：合法 `YYYYMMDDHHmmSS` + 非法时区
/// 让 `parse_tz_offset` 返 None 让 `?` bubble。
#[test]
fn parse_pdf_d_format_invalid_tz_returns_none() {
    assert!(parse_pdf_d_format("D:20170214103000+XY'00'").is_none());
}

// ============= parse_tz_offset 分支覆盖 =============

#[test]
fn parse_tz_offset_empty_returns_utc() {
    assert_eq!(parse_tz_offset(b"").unwrap().local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_z_returns_utc() {
    assert_eq!(parse_tz_offset(b"Z").unwrap().local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_positive_hh_mm() {
    let off = parse_tz_offset(b"+08'00'").unwrap();
    assert_eq!(off.local_minus_utc(), 8 * 3600);
}

#[test]
fn parse_tz_offset_negative_hh_mm() {
    let off = parse_tz_offset(b"-05'30'").unwrap();
    assert_eq!(off.local_minus_utc(), -(5 * 3600 + 30 * 60));
}

#[test]
fn parse_tz_offset_hh_only_no_mm_treats_as_zero_mm() {
    // 缺 mm 字段 → mm=0；+08 解析为 +08:00
    let off = parse_tz_offset(b"+08").unwrap();
    assert_eq!(off.local_minus_utc(), 8 * 3600);
}

#[test]
fn parse_tz_offset_unknown_sign_falls_back_to_utc() {
    // 既非 +/-/Z 也非空 → match 默认 arm 返 UTC
    let off = parse_tz_offset(b"X").unwrap();
    assert_eq!(off.local_minus_utc(), 0);
}

#[test]
fn parse_tz_offset_too_short_after_sign_returns_none() {
    // 只有 `+`，hh 字段不全 → None
    assert!(parse_tz_offset(b"+").is_none());
}

#[test]
fn parse_tz_offset_non_digit_hh_returns_none() {
    assert!(parse_tz_offset(b"+XY'00'").is_none());
}

#[test]
fn parse_tz_offset_non_digit_mm_returns_none() {
    assert!(parse_tz_offset(b"+08'XY'").is_none());
}

/// 覆盖 `from_utf8(tz.get(1..3)?).ok()?` Err arm：tz 含非 UTF-8 字节 → `from_utf8` Err。
#[test]
fn parse_tz_offset_non_utf8_hh_returns_none() {
    assert!(parse_tz_offset(b"+\xff\xff").is_none());
}

/// 覆盖 mm 解析时 `from_utf8(&mm).ok()?` Err arm：mm 字段含非 UTF-8 字节。
#[test]
fn parse_tz_offset_non_utf8_mm_returns_none() {
    let mut buf: Vec<u8> = b"+08'".to_vec();
    buf.push(0xff);
    buf.push(0xff);
    buf.push(b'\'');
    assert!(parse_tz_offset(&buf).is_none());
}

// ============= parse_pair 边界 =============

/// 覆盖 `from_utf8(slice).ok()?` Err arm：slice 含非 UTF-8 字节。
#[test]
fn parse_pair_non_utf8_bytes_returns_none() {
    let bytes = [b'D', b':', 0xff, 0xff];
    assert!(parse_pair(&bytes, 2).is_none());
}

/// 覆盖 `bytes.get(offset..offset + 2)?` Err arm：offset 超出 slice 长度。
#[test]
fn parse_pair_offset_past_end_returns_none() {
    assert!(parse_pair(b"ab", 5).is_none());
}

/// happy path：两位 ASCII 数字 → 解析为 u32。
#[test]
fn parse_pair_two_digit_ok() {
    assert_eq!(parse_pair(b"12", 0), Some(12));
}

// ============= find_subslice 边界 =============

#[test]
fn find_subslice_found() {
    assert_eq!(find_subslice(b"abcdef", b"cd"), Some(2));
}

#[test]
fn find_subslice_not_found() {
    assert!(find_subslice(b"abcdef", b"xyz").is_none());
}

// ============= scan_d_date_after_key 分支 =============

#[test]
fn scan_d_date_after_key_happy_path() {
    let buf = b"/CreationDate (D:20170214103000+08'00') ...";
    assert_eq!(
        scan_d_date_after_key(buf, b"/CreationDate"),
        Some(1_487_039_400)
    );
}

#[test]
fn scan_d_date_after_key_missing_key_returns_none() {
    let buf = b"no key here at all";
    assert!(scan_d_date_after_key(buf, b"/CreationDate").is_none());
}

#[test]
fn scan_d_date_after_key_missing_open_paren_returns_none() {
    let buf = b"/CreationDate something_no_paren_after";
    assert!(scan_d_date_after_key(buf, b"/CreationDate").is_none());
}

#[test]
fn scan_d_date_after_key_missing_close_paren_returns_none() {
    let buf = b"/CreationDate (D:20170214 no close paren ever";
    assert!(scan_d_date_after_key(buf, b"/CreationDate").is_none());
}

#[test]
fn scan_d_date_after_key_invalid_utf8_payload_returns_none() {
    // 非法 UTF-8 字节（0xff）在 (...) 内 → from_utf8 失败 → None
    let mut buf: Vec<u8> = b"/CreationDate (".to_vec();
    buf.push(0xff);
    buf.extend_from_slice(b")");
    assert!(scan_d_date_after_key(&buf, b"/CreationDate").is_none());
}

#[test]
fn scan_d_date_after_key_invalid_date_format_returns_none() {
    // key + (..) 但内容非 D: 格式 → parse_pdf_d_format 返 None
    let buf = b"/CreationDate (not a date)";
    assert!(scan_d_date_after_key(buf, b"/CreationDate").is_none());
}

// ============= parse(reader, mime) e2e =============

#[test]
fn parse_extracts_creation_and_mod_dates() {
    let pdf = b"%PDF-1.4\n3 0 obj\n<< /CreationDate (D:20170214103000+08'00') /ModDate (D:20180101120000Z) >>\nendobj\n";
    let mut r = Cursor::new(pdf.to_vec());
    let (c, m) = parse(&mut r, "application/pdf");
    assert_eq!(c, 1_487_039_400);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn parse_returns_zeros_when_no_info_dict() {
    let pdf = b"%PDF-1.4\nempty document with no /CreationDate";
    let mut r = Cursor::new(pdf.to_vec());
    assert_eq!(parse(&mut r, "application/pdf"), (0, 0));
}

#[test]
fn parse_with_creation_only_returns_zero_for_modified() {
    let pdf = b"%PDF-1.4\n3 0 obj << /CreationDate (D:20170214103000Z) >> endobj";
    let mut r = Cursor::new(pdf.to_vec());
    let (c, m) = parse(&mut r, "application/pdf");
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn parse_truncates_at_scan_limit() {
    // 把 /CreationDate 放在 PDF_SCAN_BYTES 之外 → 找不到 → (0, 0)
    let mut data = vec![0u8; PDF_SCAN_BYTES + 100];
    data.extend_from_slice(b"/CreationDate (D:20170214103000Z)");
    let mut r = Cursor::new(data);
    assert_eq!(parse(&mut r, "application/pdf"), (0, 0));
}

/// Reader read Err 时直接返 (0, 0)。用包装 reader 注入读错。
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
            Ok(0)
        }
    }

    let mut r = FailRead;
    assert_eq!(parse(&mut r, "application/pdf"), (0, 0));
}
