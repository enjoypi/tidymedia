// spec §2.P2：文件名启发式——根据来源识别四类模板。

use tidymedia::media_time::filename::parse_filename;
use tidymedia::media_time::Source;

use super::common::{east8, utc_offset};

/// spec §2.P2 主流相机：DSC_yyyymmdd_hhmmss。
#[test]
fn camera_dsc_pattern() {
    let c = parse_filename("DSC_20240501_143000.jpg", utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameCamera);
    // 默认 offset 是 UTC → 本地 14:30 UTC = epoch 1_714_573_800
    assert_eq!(c.utc.timestamp(), 1_714_573_800);
    assert!(c.inferred_offset, "spec §四：文件名无时区，offset 必为推断");
}

/// spec §2.P2 主流手机：IMG_yyyymmdd_hhmmss。
#[test]
fn phone_img_pattern() {
    let c = parse_filename("IMG_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenamePhone);
    // 本地 14:30 +08:00 = UTC 06:30
    assert_eq!(c.utc.timestamp(), 1_714_545_000);
}

/// spec §2.P2 截图：Screenshot_yyyy-mm-dd-hh-mm-ss。
#[test]
fn screenshot_pattern() {
    let c = parse_filename("Screenshot_2024-05-17-12-00-00.jpg", utc_offset()).unwrap();
    assert_eq!(c.source, Source::FilenameScreenshot);
    assert_eq!(c.utc.timestamp(), 1_715_947_200);
}

/// spec §2.P2 IM/网盘：13 位 Unix 毫秒时间戳。无时区。
#[test]
fn im_unix_millis_pattern() {
    let c = parse_filename("1715961600000.jpg", east8()).unwrap();
    assert_eq!(c.source, Source::FilenameUnixMillis);
    assert!(c.offset.is_none(), "Unix 毫秒无时区语义");
    assert!(!c.inferred_offset);
    assert_eq!(c.utc.timestamp(), 1_715_961_600);
}

/// spec §2.P2：不匹配任何模板返回 None。
#[test]
fn unknown_filename_returns_none() {
    assert!(parse_filename("vacation_photo.jpg", utc_offset()).is_none());
}

/// spec §四：文件名无时区，由调用方默认时区解释，inferred_offset 标 true。
#[test]
fn filename_inferred_offset_flagged() {
    let c = parse_filename("DSC_20240501_143000.jpg", east8()).unwrap();
    assert_eq!(c.offset, Some(east8()));
    assert!(c.inferred_offset);
}
