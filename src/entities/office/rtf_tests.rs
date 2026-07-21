//! RTF `\creatim` / `\revtim` 解析单测。

use super::*;

#[test]
fn extract_dates_happy_path() {
    let rtf = br"{\rtf1\info\creatim\yr2017\mo2\dy14\hr10\min30\sec0\revtim\yr2018\mo1\dy1\hr12\min0\sec0}";
    let (c, m) = extract_dates(rtf);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}

#[test]
fn extract_dates_missing_revtim_returns_zero() {
    let rtf = br"{\rtf1\info\creatim\yr2017\mo2\dy14\hr10\min30\sec0}";
    let (c, m) = extract_dates(rtf);
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 0);
}

#[test]
fn extract_dates_no_creatim_returns_zeros() {
    assert_eq!(extract_dates(br"{\rtf1\info}"), (0, 0));
}

// ============= scan_time_group =============

#[test]
fn scan_time_group_missing_yr_returns_none() {
    let rtf = br"\creatim\mo2\dy14";
    assert!(scan_time_group(rtf, b"\\creatim").is_none());
}

#[test]
fn scan_time_group_mo_dy_default_to_one() {
    // 只有 \yr → mo/dy 默认 1, hr/min/sec 默认 0 → 2017-01-01 00:00:00 UTC
    let rtf = br"\creatim\yr2017";
    let result = scan_time_group(rtf, b"\\creatim");
    assert_eq!(result, Some(1_483_228_800));
}

#[test]
fn scan_time_group_invalid_date_returns_none() {
    // 月 13 越界 → NaiveDate::from_ymd_opt 返 None。
    let rtf = br"\creatim\yr2017\mo13\dy1";
    assert!(scan_time_group(rtf, b"\\creatim").is_none());
}

#[test]
fn scan_time_group_invalid_time_returns_none() {
    // 时 25 越界 → and_hms_opt 返 None。
    let rtf = br"\creatim\yr2017\mo2\dy14\hr25";
    assert!(scan_time_group(rtf, b"\\creatim").is_none());
}

#[test]
fn scan_time_group_pre_epoch_returns_none() {
    let rtf = br"\creatim\yr1969\mo1\dy1";
    assert!(scan_time_group(rtf, b"\\creatim").is_none());
}

#[test]
fn scan_time_group_no_tag_returns_none() {
    assert!(scan_time_group(br"no creatim here", b"\\creatim").is_none());
}

// ============= scan_int_after =============

#[test]
fn scan_int_after_positive() {
    let r: Option<i32> = scan_int_after(br"\yr2017\mo", b"\\yr");
    assert_eq!(r, Some(2017));
}

#[test]
fn scan_int_after_negative() {
    let r: Option<i32> = scan_int_after(br"\yr-100\mo", b"\\yr");
    assert_eq!(r, Some(-100));
}

#[test]
fn scan_int_after_only_dash_returns_none() {
    // `\yr-` 后无数字 → None。
    let r: Option<i32> = scan_int_after(br"\yr-\mo", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn scan_int_after_no_digits_returns_none() {
    let r: Option<i32> = scan_int_after(br"\yrXY\mo", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn scan_int_after_key_missing_returns_none() {
    let r: Option<i32> = scan_int_after(br"\mo2", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn scan_int_after_empty_after_returns_none() {
    let r: Option<i32> = scan_int_after(br"\yr", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn scan_int_after_overflow_returns_none() {
    // 超过 i32 range 让 parse Err。
    let r: Option<i32> = scan_int_after(br"\yr99999999999", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn scan_int_after_negative_only_no_more_digits() {
    // 仅 `-` 没数字。
    let r: Option<i32> = scan_int_after(b"\\yr-", b"\\yr");
    assert_eq!(r, None);
}

#[test]
fn find_subslice_basic() {
    assert_eq!(find_subslice(b"hello world", b"world"), Some(6));
    assert!(find_subslice(b"hello", b"world").is_none());
}

// ============= group bounding =============

#[test]
fn scan_time_group_missing_yr_does_not_leak_to_next_group() {
    // \creatim 组内无 \yr，但紧跟的 \revtim 组含 \yr2024：组边界让前者返 None
    // 而不是错读 2024 作为创建年（旧无界扫描的 bug）。
    let rtf = br"{\rtf1\info{\creatim\mo6\dy15}{\revtim\yr2024\mo1\dy1}}";
    let (created, modified) = extract_dates(rtf);
    assert_eq!(
        created, 0,
        "\\creatim 缺 \\yr → 不应跨段拾取 \\revtim 的 2024"
    );
    assert!(modified > 0, "\\revtim 自己的 \\yr2024 仍可读出");
}

#[test]
fn scan_time_group_creatim_mo_default_when_next_group_has_mo() {
    // \creatim 仅给 \yr2017；mo/dy 默认 1（而不是跨段读 \revtim 的 mo6）。
    let rtf = br"{\creatim\yr2017}{\revtim\yr2018\mo6\dy15}";
    let result = scan_time_group(rtf, b"\\creatim");
    // 2017-01-01 00:00:00 UTC
    assert_eq!(result, Some(1_483_228_800));
}

#[test]
fn find_group_end_handles_nested_braces() {
    // depth 计数：嵌套 `{...}` 不应误判外层 `}` 为结束。
    let buf = br"abc{xy}def}rest";
    assert_eq!(find_group_end(buf, 0), 10);
}

#[test]
fn find_group_end_skips_rtf_escape() {
    // RTF 转义 `\{` `\}` `\\` 不参与 brace depth 计数。
    let buf = br"\{\}\\}rest";
    assert_eq!(find_group_end(buf, 0), 6);
}

#[test]
fn find_group_end_unterminated_returns_buf_len() {
    let buf = br"abc no close";
    assert_eq!(find_group_end(buf, 0), buf.len());
}

#[test]
fn find_group_end_backslash_followed_by_non_escape_is_literal() {
    // buf[i]=b'\\' 但 buf[i+1] 非 `{` / `}` / `\\` → `matches!` False → 落
    // `_ => {}` catch-all；覆盖 L70 `matches!(buf[i+1], b'{'|b'}'|b'\\')` 的 False 分支。
    let buf = br"\a}rest";
    assert_eq!(find_group_end(buf, 0), 2);
}

#[test]
fn find_group_end_trailing_backslash_no_next_byte() {
    // buf 末尾单个 `\`，i+1 >= buf.len() → guard 短路 False → catch-all；
    // 覆盖 `i + 1 < buf.len()` 短路 False 分支。
    let buf = b"a\\";
    assert_eq!(find_group_end(buf, 0), buf.len());
}

// ============= strip_rtf_into（extract_text 业务） =============

#[test]
fn strip_rtf_extracts_plain_text() {
    let mut out = String::new();
    strip_rtf_into(b"{\\rtf1\\ansi Hello World}", &mut out, 64);
    assert_eq!(out, "Hello World");
}

#[test]
fn strip_rtf_decodes_unicode_escape_chinese() {
    // 发=U+53D1=21457 票=U+7968=31080；`\uN?` 的 fallback `?` 必须被吞。
    let mut out = String::new();
    strip_rtf_into(b"{\\rtf1 \\u21457?\\u31080?}", &mut out, 64);
    assert_eq!(out, "发票");
}

#[test]
fn strip_rtf_unicode_negative_wraps_to_bmp() {
    // \u-10179 → -10179 + 65536 = 55357 落 surrogate 区 → from_u32 None → 丢弃。
    let mut out = String::new();
    strip_rtf_into(b"{\\u-10179?}x", &mut out, 64);
    assert_eq!(out, "x");
}

#[test]
fn strip_rtf_skips_fonttbl_group() {
    let mut out = String::new();
    strip_rtf_into(b"{\\rtf1{\\fonttbl{\\f0 Arial;}}body}", &mut out, 64);
    assert_eq!(out, "body");
}

#[test]
fn strip_rtf_skips_star_extension_group() {
    let mut out = String::new();
    strip_rtf_into(b"{\\rtf1{\\*\\generator Foo 1.0;}text}", &mut out, 64);
    assert_eq!(out, "text");
}

#[test]
fn strip_rtf_par_becomes_space() {
    let mut out = String::new();
    strip_rtf_into(b"{a\\par b}", &mut out, 64);
    assert_eq!(out, "a b");
}

#[test]
fn strip_rtf_hex_escape_ascii() {
    let mut out = String::new();
    strip_rtf_into(b"{\\'41\\'42}", &mut out, 64);
    assert_eq!(out, "AB");
}

#[test]
fn strip_rtf_hex_escape_invalid_digits_skipped() {
    let mut out = String::new();
    strip_rtf_into(b"{\\'zz ok}", &mut out, 64);
    assert_eq!(out, "zz ok");
}

#[test]
fn strip_rtf_escaped_braces_literal() {
    let mut out = String::new();
    strip_rtf_into(b"{\\{x\\}}", &mut out, 64);
    assert_eq!(out, "{x}");
}

#[test]
fn strip_rtf_symbol_control_becomes_space() {
    let mut out = String::new();
    strip_rtf_into(b"{a\\~b}", &mut out, 64);
    assert_eq!(out, "a b");
}

#[test]
fn strip_rtf_budget_truncates() {
    let mut out = String::new();
    strip_rtf_into(b"{abcdefgh}", &mut out, 4);
    assert_eq!(out, "abcd");
}

#[test]
fn strip_rtf_trailing_backslash_is_safe() {
    let mut out = String::new();
    strip_rtf_into(b"{ab}\\", &mut out, 64);
    assert_eq!(out, "ab");
}

#[test]
fn strip_rtf_escaped_brace_inside_skip_group_not_terminating() {
    // skip group 内 `\}` 转义不得当组闭合；组后正文保留。
    let mut out = String::new();
    strip_rtf_into(b"{\\info \\} still}after", &mut out, 64);
    assert_eq!(out, "after");
}

#[test]
fn strip_rtf_u_without_digits_ignored() {
    let mut out = String::new();
    strip_rtf_into(b"{\\u x}", &mut out, 64);
    assert_eq!(out, "x");
}

#[test]
fn strip_rtf_control_word_with_numeric_param_dropped() {
    let mut out = String::new();
    strip_rtf_into(b"{\\fs24 sized}", &mut out, 64);
    assert_eq!(out, "sized");
}

#[test]
fn match_skip_group_none_for_body_word() {
    assert!(match_skip_group(b"\\b bold").is_none());
}

#[test]
fn hex_val_covers_ranges() {
    assert_eq!(hex_val(b'0'), Some(0));
    assert_eq!(hex_val(b'a'), Some(10));
    assert_eq!(hex_val(b'F'), Some(15));
    assert_eq!(hex_val(b'g'), None);
}

#[test]
fn consume_control_empty_rest_returns_zero() {
    let mut text = Vec::new();
    assert_eq!(consume_control(b"", &mut text), 0);
    assert!(text.is_empty());
}
