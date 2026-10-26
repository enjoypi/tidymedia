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
#[case::screenshot_samsung_15char_no_ext(
    "Screenshot_20240517_120000",
    epoch("2024-05-17T12:00:00Z")
)]
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
// macOS Snipping：`Screen Shot YYYY-MM-DD at HH.MM.SS.png`
#[case::macos_basic(
    "Screen Shot 2024-05-17 at 12.00.00.png",
    epoch("2024-05-17T12:00:00Z")
)]
// 带序号后缀 ` (2)` 不影响前 22 字符解析
#[case::macos_with_seq(
    "Screen Shot 2024-05-17 at 12.00.00 (2).png",
    epoch("2024-05-17T12:00:00Z")
)]
// `.jpg` 扩展名（部分版本/手动转格式）
#[case::macos_jpg(
    "Screen Shot 2024-05-17 at 12.00.00.jpg",
    epoch("2024-05-17T12:00:00Z")
)]
fn macos_screenshot_parsed(#[case] name: &str, #[case] expected_ts: i64) {
    let c = parse_filename(name, utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameScreenshot);
    assert_eq!(c.utc.timestamp(), expected_ts);
    assert!(c.inferred_offset);
}

#[rstest]
// 缺 `at` 分隔
#[case::macos_no_at("Screen Shot 2024-05-17    12.00.00.png")]
// 非法月份
#[case::macos_bad_month("Screen Shot 2024-13-17 at 12.00.00.png")]
// 前缀错（少空格）
#[case::macos_wrong_prefix("ScreenShot 2024-05-17 at 12.00.00.png")]
// 长度不足 22
#[case::macos_short("Screen Shot 2024-05-17.png")]
fn macos_screenshot_rejects(#[case] name: &str) {
    assert!(parse_filename(name, utc_offset()).is_none());
}

#[test]
fn macos_screenshot_east8_offset_applied() {
    // 本地 12:00 +08:00 = UTC 04:00
    let c = parse_filename("Screen Shot 2024-05-17 at 12.00.00.png", east8()).unwrap();
    let expected = Utc.with_ymd_and_hms(2024, 5, 17, 4, 0, 0).unwrap();
    assert_eq!(c.utc, expected);
    assert_eq!(c.offset, Some(east8()));
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
