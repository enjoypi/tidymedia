use tidymedia::{
    rtf_consume_control, rtf_extract_text, rtf_parse, rtf_scan_int_after, rtf_strip_rtf_into,
};

#[derive(Debug)]
struct FailingReader;

impl std::io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("injected read failure"))
    }
}

impl std::io::Seek for FailingReader {
    fn seek(&mut self, _p: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::other("injected seek failure"))
    }
}

#[test]
fn parse_read_error_returns_zero() {
    assert_eq!(rtf_parse(&mut FailingReader, "text/rtf"), (0, 0));
}

#[test]
fn extract_text_read_error_returns_empty() {
    assert_eq!(rtf_extract_text(&mut FailingReader, "text/rtf", 64), "");
}

#[test]
fn strip_rtf_trailing_backslash_in_skip_group_is_safe() {
    let mut out = String::new();
    rtf_strip_rtf_into(b"{\\info abc\\", &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn strip_rtf_crlf_dropped_from_text() {
    let mut out = String::new();
    rtf_strip_rtf_into(b"a\r\nb", &mut out, 64);
    assert_eq!(out, "ab");
}

fn consume(rest: &[u8]) -> (usize, Vec<u8>) {
    let mut text = Vec::new();
    let n = rtf_consume_control(rest, &mut text);
    (n, text)
}

#[test]
fn consume_control_escapes_and_hex() {
    assert_eq!(consume(b"{"), (1, b"{".to_vec()));
    assert_eq!(consume(b"~"), (1, b" ".to_vec()));
    assert_eq!(consume(b"'41x"), (3, b"A".to_vec()));
    assert_eq!(consume(b"'4"), (1, Vec::new()));
    assert_eq!(consume(b"'"), (1, Vec::new()));
}

#[test]
fn consume_control_word_running_to_buffer_end() {
    assert_eq!(consume(b"par"), (3, b" ".to_vec()));
    assert_eq!(consume(b"fs24"), (4, Vec::new()));
    assert_eq!(consume(b"fs24 x"), (5, Vec::new()));
}

#[test]
fn consume_control_unicode_param_variants() {
    assert_eq!(consume(b"u21457 rest"), (7, "发".as_bytes().to_vec()));
    assert_eq!(consume(b"u21457?x"), (7, "发".as_bytes().to_vec()));
    assert_eq!(consume(b"u21457x"), (6, "发".as_bytes().to_vec()));
    assert_eq!(consume(b"u21457"), (6, "发".as_bytes().to_vec()));
    assert_eq!(consume(b"u-10179?x"), (8, Vec::new()));
}

#[test]
fn scan_int_after_i32_covers_all_branches() {
    assert_eq!(
        rtf_scan_int_after::<i32>(br"\yr2017\mo", b"\\yr"),
        Some(2017)
    );
    assert_eq!(rtf_scan_int_after::<i32>(br"\yr-100", b"\\yr"), Some(-100));
    assert_eq!(rtf_scan_int_after::<i32>(br"\mo2", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<i32>(br"\yr", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<i32>(br"\yr-", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<i32>(br"\yrXY", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<i32>(br"\yr4x", b"\\yr"), Some(4));
    assert_eq!(rtf_scan_int_after::<i32>(br"\yr99999999999", b"\\yr"), None);
}

#[test]
fn scan_int_after_u32_covers_all_branches() {
    assert_eq!(
        rtf_scan_int_after::<u32>(br"\yr2017\mo", b"\\yr"),
        Some(2017)
    );
    assert_eq!(rtf_scan_int_after::<u32>(br"\mo2", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<u32>(br"\yr", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<u32>(br"\yr-", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<u32>(br"\yr-100", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<u32>(br"\yrXY", b"\\yr"), None);
    assert_eq!(rtf_scan_int_after::<u32>(br"\yr4x", b"\\yr"), Some(4));
    assert_eq!(rtf_scan_int_after::<u32>(br"\yr99999999999", b"\\yr"), None);
}
