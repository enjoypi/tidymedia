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
