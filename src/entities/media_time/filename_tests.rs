use chrono::TimeZone;
use chrono::Utc;
use rstest::rstest;

use super::*;

fn east8() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

/// UTC epoch at a given date-time string (RFC3339 without sub-seconds).
fn epoch(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}

#[rstest]
#[case::img_basic(
    "IMG_20240501_143000.jpg",
    Source::FilenamePhone,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::img_no_ext(
    "IMG_20240501_143000",
    Source::FilenamePhone,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::dsc_basic(
    "DSC_20240501_143000.jpg",
    Source::FilenameCamera,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::dsc_no_ext(
    "DSC_20240501_143000",
    Source::FilenameCamera,
    epoch("2024-05-01T14:30:00Z")
)]
#[case::vid_basic(
    "VID_20230615_103000.mp4",
    Source::FilenameVideoPhone,
    epoch("2023-06-15T10:30:00Z")
)]
#[case::vid_no_ext(
    "VID_20230615_103000",
    Source::FilenameVideoPhone,
    epoch("2023-06-15T10:30:00Z")
)]
fn camera_phone_video_parsed_utc(
    #[case] name: &str,
    #[case] expected_source: Source,
    #[case] expected_ts: i64,
) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, expected_source);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
    assert_eq!(c.offset, Some(utc_offset()));
}

#[test]
fn img_east8_offset_applied() {
    // 本地 14:30 +08:00 = UTC 06:30
    let c = parse_filename("IMG_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenamePhone);
    assert_eq!(c.offset, Some(east8()));
    assert_eq!(c.utc.timestamp(), epoch("2024-05-01T06:30:00Z"));
}

#[rstest]
// wrong suffix length
#[case::img_short("IMG_2024050_143000.jpg")]
// bad month 13
#[case::img_bad_month("IMG_20241332_143000.jpg")]
// VID bad day 32
#[case::vid_bad_day("VID_20231532_103000.mp4")]
// 注：`DSC_202405011_143000.jpg` 这种「DSC_ 模板坏掉但 stem 含合法 8 位日期」
// 现在被 `try_loose_yyyymmdd` 兜底命中（见 `loose_yyyymmdd_parsed`），不再返回 None。
fn camera_phone_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[rstest]
#[case::pxl_basic("PXL_20230615_103045123.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_mp("PXL_20230615_103045123.MP.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_portrait("PXL_20230615_103045123.PORTRAIT.jpg", epoch("2023-06-15T10:30:45Z"))]
#[case::pxl_no_ext("PXL_20230615_103045123", epoch("2023-06-15T10:30:45Z"))]
// 恰好 15 字符（无毫秒后缀）：杀 `len < 15` 被变异成 `<= 15`（边界值被误拒）
#[case::pxl_exact_15_no_millis("PXL_20230615_103045.jpg", epoch("2023-06-15T10:30:45Z"))]
fn pixel_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenamePixel);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// too short after PXL_ prefix
#[case::pxl_short("PXL_2023061.jpg")]
// 注：`PXL_20230615_993045123.jpg`（坏掉的时分秒 99:30:45）现被 `try_loose_yyyymmdd`
// 兜底命中为 2023-06-15 仅日期粒度的 candidate，不再返回 None。
fn pixel_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[rstest]
#[case::screenshot_basic("Screenshot_2024-05-17-12-00-00.jpg", epoch("2024-05-17T12:00:00Z"))]
#[case::screenshot_no_ext("Screenshot_2024-05-17-12-00-00", epoch("2024-05-17T12:00:00Z"))]
// Samsung / MIUI / 原生 Android 15-char 模板：`Screenshot_yyyymmdd_HHMMSS`
// 命中 try_screenshot 第二个 else if 分支（rest.len() >= 15 && parse Ok），
// 旧测试集只覆盖 19-char dash 形式，导致 BRDA:177,0,0 永不命中。
#[case::screenshot_samsung_15char("Screenshot_20240517_120000.jpg", epoch("2024-05-17T12:00:00Z"))]
#[case::screenshot_samsung_15char_no_ext("Screenshot_20240517_120000", epoch("2024-05-17T12:00:00Z"))]
fn screenshot_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameScreenshot);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// wrong length
#[case::screenshot_short("Screenshot_2024-05-17.jpg")]
// bad month 13
#[case::screenshot_bad_month("Screenshot_2024-13-32-25-99-99.jpg")]
fn screenshot_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[rstest]
#[case::mmexport_basic("mmexport1686824625000.jpg", 1_686_824_625)]
#[case::mmexport_no_ext("mmexport1686824625000", 1_686_824_625)]
fn mmexport_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameWeChatExport);
    assert_eq!(c.offset, None);
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// 12 digits (too short)
#[case::mmexport_short("mmexport168682462500.jpg")]
// non-digit
#[case::mmexport_alpha("mmexport168682462500a.jpg")]
// empty after prefix
#[case::mmexport_empty("mmexport.jpg")]
fn mmexport_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[rstest]
#[case::wa_image(
    "WhatsApp Image 2023-06-15 at 10.30.45.jpeg",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_video(
    "WhatsApp Video 2023-06-15 at 10.30.45.mp4",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_image_seq(
    "WhatsApp Image 2023-06-15 at 10.30.45 (1).jpeg",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
#[case::wa_video_seq(
    "WhatsApp Video 2023-06-15 at 10.30.45 (2).mp4",
    Source::FilenameWhatsApp,
    epoch("2023-06-15T10:30:45Z")
)]
fn whatsapp_parsed(#[case] name: &str, #[case] expected_source: Source, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, expected_source);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// too short (missing time part)
#[case::wa_short("WhatsApp Image 2023-06-15.jpeg")]
// bad month 13
#[case::wa_bad_month("WhatsApp Image 2023-13-15 at 10.30.45.jpeg")]
// unrecognized prefix
#[case::wa_wrong_prefix("WhatsApp Audio 2023-06-15 at 10.30.45.m4a")]
fn whatsapp_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[rstest]
#[case::bare_basic("20230615_103000.jpg", epoch("2023-06-15T10:30:00Z"))]
#[case::bare_no_ext("20230615_103000", epoch("2023-06-15T10:30:00Z"))]
fn bare_yyyymmdd_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameBareYyyymmdd);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// bad month 13
#[case::bare_bad_month("20231332_103000.jpg")]
// wrong length (14 chars)
#[case::bare_short("2023061_103000.jpg")]
// looks like unix millis (13 digits) — should match unix_millis not bare
#[case::bare_vs_millis("1686824625000.jpg")]
fn bare_yyyymmdd_rejects_or_routes_elsewhere(#[case] name: &str) {
    // For the millis case, it should still parse (via unix_millis), just not as bare
    if name == "1686824625000.jpg" {
        let c = parse_filename(name, utc_offset()).unwrap();
        assert_eq!(c.source, Source::FilenameUnixMillis);
    } else {
        assert!(parse_filename(name, utc_offset()).is_none());
    }
}

#[rstest]
#[case::millis_basic("1715961600000.jpg", 1_715_961_600)]
#[case::millis_no_ext("1715961600000", 1_715_961_600)]
fn unix_millis_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameUnixMillis);
    assert_eq!(c.offset, None);
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), expected_ts);
}

#[rstest]
// 12 digits
#[case::millis_12("171596160000.jpg")]
// 14 digits
#[case::millis_14("17159616000000.jpg")]
// alpha char
#[case::millis_alpha("171596160000a.jpg")]
fn unix_millis_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

// 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS` 模板：相机时钟错误时，
// 事后批量重命名工具写入文件名的时间往往才是真实拍摄时间。
#[rstest]
#[case::chinese_prefix("三星堆 2002-12-25 22-29-00.jpg", epoch("2002-12-25T22:29:00Z"))]
#[case::ascii_prefix("trip 2010-01-02 03-04-05.png", epoch("2010-01-02T03:04:05Z"))]
// 无前缀：stem 恰好 19 chars，杀窗口起点 off-by-one 变异
#[case::no_prefix("2002-12-25 22-29-00.jpg", epoch("2002-12-25T22:29:00Z"))]
// 带后缀：日期时间不在 stem 末尾
#[case::with_suffix("三星堆 2002-12-25 22-29-00 副本.jpg", epoch("2002-12-25T22:29:00Z"))]
fn generic_dashed_datetime_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameDashedDateTime);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

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
