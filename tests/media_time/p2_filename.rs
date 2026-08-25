use tidymedia::media_time::Source;
use tidymedia::media_time::filename::parse_filename;

use super::common::{east8, utc_offset};

/// 主流相机：`DSC_yyyymmdd_hhmmss`。
#[test]
fn camera_dsc_pattern() {
    let c = parse_filename("DSC_20240501_143000.jpg", utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameCamera);
    // 默认 offset 是 UTC → 本地 14:30 UTC = epoch 1_714_573_800
    assert_eq!(c.utc.timestamp(), 1_714_573_800);
    assert!(c.inferred_offset, "文件名无时区，offset 必为推断");
}

/// 主流手机：`IMG_yyyymmdd_hhmmss`。
#[test]
fn phone_img_pattern() {
    let c = parse_filename("IMG_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenamePhone);
    // 本地 14:30 +08:00 = UTC 06:30
    assert_eq!(c.utc.timestamp(), 1_714_545_000);
}

/// 截图：`Screenshot_yyyy-mm-dd-hh-mm-ss`。
#[test]
fn screenshot_pattern() {
    let c = parse_filename("Screenshot_2024-05-17-12-00-00.jpg", utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameScreenshot);
    assert_eq!(c.utc.timestamp(), 1_715_947_200);
}

/// IM/网盘：13 位 Unix 毫秒时间戳。无时区。
#[test]
fn im_unix_millis_pattern() {
    let c = parse_filename("1715961600000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameUnixMillis);
    assert!(c.offset.is_none(), "Unix 毫秒无时区语义");
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), 1_715_961_600);
}

/// 不匹配任何模板返回 None。
#[test]
fn unknown_filename_returns_none() {
    assert!(parse_filename("vacation_photo.jpg", utc_offset()).is_none());
}

/// 文件名无时区，由调用方默认时区解释，`inferred_offset` 标 true。
#[test]
fn filename_inferred_offset_flagged() {
    let c = parse_filename("DSC_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.offset, Some(east8()));
    assert!(c.inferred_offset);
}

/// 安卓视频：`VID_yyyymmdd_HHMMSS`。
#[test]
fn video_phone_vid_pattern() {
    let c = parse_filename("VID_20230615_103000.mp4", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameVideoPhone);
    // 本地 10:30 +08:00 = UTC 02:30；2023-06-15 02:30 UTC = 1_686_796_200。
    assert_eq!(c.utc.timestamp(), 1_686_796_200);
    assert_eq!(c.offset, Some(east8()));
    assert!(c.inferred_offset);
}

/// Google Pixel：`PXL_yyyymmdd_HHMMSSmmm[.MP][.PORTRAIT]`；尾部毫秒 / `.MP` 后缀丢弃。
#[test]
fn pixel_pattern_strips_millis_and_suffix() {
    let c = parse_filename("PXL_20240115_103045123.MP.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenamePixel);
    // 本地 10:30:45 +08:00 = UTC 02:30:45；2024-01-15 02:30:45 UTC = 1_705_285_845。
    assert_eq!(c.utc.timestamp(), 1_705_285_845);
    assert!(c.inferred_offset);
}

/// 微信导出：`mmexport<13-digit-ms>`；无时区，毫秒戳直接当 UTC。
#[test]
fn wechat_mmexport_pattern() {
    let c = parse_filename("mmexport1686824625000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameWeChatExport);
    assert!(c.offset.is_none(), "mmexport 13 位毫秒无时区语义");
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), 1_686_824_625);
}

/// `WhatsApp`：`WhatsApp {Image|Video} YYYY-MM-DD at HH.MM.SS[ (N)]`；本地时间。
#[test]
fn whatsapp_image_pattern_with_seq() {
    let c = parse_filename("WhatsApp Image 2023-06-15 at 10.30.45 (1).jpeg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameWhatsApp);
    // 本地 10:30:45 +08:00 = UTC 02:30:45；2023-06-15 02:30:45 UTC = 1_686_796_245。
    assert_eq!(c.utc.timestamp(), 1_686_796_245);
    assert!(
        c.inferred_offset,
        "WhatsApp 写本地时间，offset 由 default 推断"
    );
}

/// 裸格式：`YYYYMMDD_HHMMSS`（无前缀）；本地时间。
#[test]
fn bare_yyyymmdd_pattern() {
    let c = parse_filename("20230615_103000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameBareYyyymmdd);
    // 本地 10:30 +08:00 = UTC 02:30；2023-06-15 02:30 UTC = 1_686_796_200。
    assert_eq!(c.utc.timestamp(), 1_686_796_200);
    assert!(c.inferred_offset);
}

/// 括号内紧凑时戳（原 IMG 拍摄命名被清理工具污染）：`IMG_6489(20210611-174530)(1).jpg`。
#[test]
fn bracketed_compact_pattern() {
    let c = parse_filename("IMG_6489(20210611-174530)(1).jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameBracketedCompact);
    // 本地 17:45:30 +08:00 = UTC 09:45:30；2021-06-11 09:45:30 UTC = 1_623_404_730。
    assert_eq!(c.utc.timestamp(), 1_623_404_730);
    assert!(c.inferred_offset);
}

/// QQ 导出：`QQ图片<14-digit YYYYMMDDHHMMSS>`；本地时间。
#[test]
fn qq_export_pattern() {
    let c = parse_filename("QQ图片20210428220203.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameQqExport);
    // 本地 22:02:03 +08:00 = UTC 14:02:03；2021-04-28 14:02:03 UTC = 1_619_618_523。
    assert_eq!(c.utc.timestamp(), 1_619_618_523);
    assert!(c.inferred_offset);
}
