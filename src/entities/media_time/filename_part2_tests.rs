#[test]
fn generic_dashed_datetime_east8_offset_applied() {
    let c = parse_filename("三星堆 2002-12-25 22-29-00.jpg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2002, 12, 25, 14, 29, 0).unwrap();
    assert_eq!(c.utc, expected);
}

#[rstest]
// bad month 13
#[case::bad_month("三星堆 2002-13-25 22-29-00.jpg")]
// bad hour 99
#[case::bad_hour("三星堆 2002-12-25 99-29-00.jpg")]
// 分隔符形状不符（日期用点号）
#[case::wrong_separator("三星堆 2002.12.25 22-29-00.jpg")]
// stem 不足 19 chars
#[case::too_short("2002-12-25.jpg")]
fn generic_dashed_datetime_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[test]
fn no_known_pattern_returns_none() {
    assert!(parse_filename("random.jpg", utc_offset()).is_none());
}

#[test]
fn empty_name_returns_none() {
    assert!(parse_filename("", utc_offset()).is_none());
}

// 括号内紧凑时戳（原 IMG 拍摄命名被清理/备份工具加括号时戳污染）。
#[rstest]
#[case::img_bracketed("IMG_6489(20210611-174530)(1).jpg", epoch("2021-06-11T17:45:30Z"))]
#[case::img_bracketed_no_ext("IMG_6489(20210611-174530)(1)", epoch("2021-06-11T17:45:30Z"))]
// 首个括号非日期、第二个是：`photo(1)(20210611-174530).jpg` → 命中后者
#[case::first_paren_not_date("photo(1)(20210611-174530).jpg", epoch("2021-06-11T17:45:30Z"))]
fn bracketed_compact_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameBracketedCompact);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[test]
fn bracketed_compact_east8_offset_applied() {
    // 本地 17:45:30 +08:00 = UTC 09:45:30
    let c = parse_filename("IMG_6489(20210611-174530)(1).jpg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2021, 6, 11, 9, 45, 30).unwrap();
    assert_eq!(c.utc, expected);
    assert_eq!(c.offset, Some(east8()));
}

#[rstest]
// 无括号
#[case::no_paren("IMG_6489.jpg")]
// 括号内容不足 15 字节窗口
#[case::short_paren("a(1)b.jpg")]
// 括号内容非数字
#[case::alpha_paren("x(abcd).jpg")]
// 括号未闭合
#[case::unclosed_paren("x(20210611-174530")]
// 括号内分隔符形状错（日期用 `_`）
#[case::wrong_sep("x(20210611_174530).jpg")]
// 形状合法但日期非法（月 99）
#[case::invalid_date("x(20239999-999999).jpg")]
fn bracketed_compact_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// QQ 导出：`QQ图片<14-digit YYYYMMDDHHMMSS>`。
#[rstest]
#[case::qq_basic("QQ图片20210428220203.jpg", epoch("2021-04-28T22:02:03Z"))]
#[case::qq_no_ext("QQ图片20210428220203", epoch("2021-04-28T22:02:03Z"))]
fn qq_export_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameQqExport);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[test]
fn qq_export_east8_offset_applied() {
    // 本地 22:02:03 +08:00 = UTC 14:02:03
    let c = parse_filename("QQ图片20210428220203.jpg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2021, 4, 28, 14, 2, 3).unwrap();
    assert_eq!(c.utc, expected);
    assert_eq!(c.offset, Some(east8()));
}

#[rstest]
// 13 位（短）
#[case::qq_short("QQ图片2021042822020.jpg")]
// 15 位（长）
#[case::qq_long("QQ图片202104282202030.jpg")]
// 含字母
#[case::qq_alpha("QQ图片2021042822020a.jpg")]
// 14 位全数字但日期非法（月 13）
#[case::qq_invalid_date("QQ图片20211301120000.jpg")]
// 前缀后不足 14 位
#[case::qq_too_short("QQ图片202104.jpg")]
fn qq_export_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[test]
fn stem_strips_last_extension_only() {
    // PXL with .MP.jpg — stem_without_ext strips only .jpg, then try_pixel strips .MP via prefix-then-15chars
    // Validate stem is "PXL_20230615_103045123.MP"
    let stem = super::stem_without_ext("PXL_20230615_103045123.MP.jpg");
    assert_eq!(stem, "PXL_20230615_103045123.MP");
}

#[test]
fn vid_east8_offset_applied() {
    let c = parse_filename("VID_20230615_103000.mp4", east8()).unwrap();
    // 本地 10:30 +08:00 = UTC 02:30
    let expected = Utc.with_ymd_and_hms(2023, 6, 15, 2, 30, 0).unwrap();
    assert_eq!(c.utc, expected);
}

#[test]
fn whatsapp_east8_offset_applied() {
    let c = parse_filename("WhatsApp Image 2023-06-15 at 10.30.45.jpeg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2023, 6, 15, 2, 30, 45).unwrap();
    assert_eq!(c.utc, expected);
}

#[rstest]
// stem 开头 8 位日期 + 后续随意 (用户场景：手机相册命名 YYYYMMDD<序号>)
#[case::head_pure_digits("20061101639532011.jpg", epoch("2006-11-01T00:00:00Z"))]
// `-` 分隔符后 8 位日期 (用户场景：<编号>-YYYYMMDD_<序号>)
#[case::dash_anchor("017-20051011_154652482.jpg", epoch("2005-10-11T00:00:00Z"))]
// `_` 分隔符后 8 位日期 (注：`abc_20060101` 长度 12 ≠ 15，不会被 try_bare_yyyymmdd 拦截)
#[case::underscore_anchor("abc_20060101.jpg", epoch("2006-01-01T00:00:00Z"))]
// 空格分隔符后
#[case::space_anchor("photo 20240315 ok.jpg", epoch("2024-03-15T00:00:00Z"))]
// 开头位置日期非法 → 跳过第 0 锚点，命中 `-` 后的合法日期
#[case::first_anchor_invalid_second_ok("99999999-20240315_x.jpg", epoch("2024-03-15T00:00:00Z"))]
// 「DSC_/PXL_ 严格模板坏掉但 stem 含合法 8 位日期」原来返 None，现兜底命中为日期 candidate
#[case::dsc_bad_template("DSC_202405011_143000.jpg", epoch("2024-05-01T00:00:00Z"))]
#[case::pxl_bad_time("PXL_20230615_993045123.jpg", epoch("2023-06-15T00:00:00Z"))]
fn loose_yyyymmdd_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameBareYyyymmdd);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// 日期数字不在锚点位置 (非分隔符前导)
#[case::digits_in_middle("abc20060101.jpg")]
// 月份越界
#[case::bad_month("20061301_x.jpg")]
// 日越界
#[case::bad_day("20060230_x.jpg")]
// stem 长度 < 8
#[case::too_short("123.jpg")]
// 锚点后不足 8 位
#[case::short_after_anchor("ab-1234.jpg")]
// 锚点后含非数字
#[case::non_digit_after_anchor("ab-2006abcd.jpg")]
// 无前缀且首字符非数字
#[case::no_anchor("hello.jpg")]
fn loose_yyyymmdd_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[test]
fn loose_yyyymmdd_east8_offset_applied() {
    // 文件名仅日期粒度 → 本地 00:00 +08:00 = UTC 前一天 16:00
    let c = parse_filename("20240315_id.jpg", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2024, 3, 14, 16, 0, 0).unwrap();
    assert_eq!(c.utc, expected);
    assert_eq!(c.offset, Some(east8()));
}
