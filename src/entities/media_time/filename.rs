// docs/media-time-detection.md §二.P2：文件名启发式。
// 支持四类常见模板；未匹配返回 None。
// 文件名提取的时间通常无时区，调用方传入的 default_offset 当本地时区参与解释，
// 并把 `inferred_offset` 标为 true（spec §四）。

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::NaiveDateTime;
use chrono::TimeDelta;
use chrono::TimeZone;
use chrono::Utc;

use super::candidate::Candidate;
use super::priority::Source;

const PHONE_PREFIX: &str = "IMG_";
const CAMERA_PREFIX: &str = "DSC_";
const SCREENSHOT_PREFIX: &str = "Screenshot_";

/// 解析 `path.file_name()`（不含目录），匹配则返回 P2 候选。
pub fn parse_filename(name: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let stem = stem_without_ext(name);
    if let Some(c) = parse_camera_or_phone(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = parse_screenshot(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = parse_unix_millis(stem) {
        return Some(c);
    }
    None
}

fn stem_without_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

fn parse_camera_or_phone(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let (rest, source) = if let Some(r) = stem.strip_prefix(PHONE_PREFIX) {
        (r, Source::FilenamePhone)
    } else if let Some(r) = stem.strip_prefix(CAMERA_PREFIX) {
        (r, Source::FilenameCamera)
    } else {
        return None;
    };
    // 期望格式：yyyymmdd_HHMMSS（8 + 1 + 6 = 15 chars）
    if rest.len() != 15 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(rest, "%Y%m%d_%H%M%S").ok()?;
    Some(naive_to_candidate(naive, default_offset, source))
}

fn parse_screenshot(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let rest = stem.strip_prefix(SCREENSHOT_PREFIX)?;
    // 期望格式：yyyy-mm-dd-HH-mm-ss（19 chars）
    if rest.len() != 19 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(rest, "%Y-%m-%d-%H-%M-%S").ok()?;
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenameScreenshot,
    ))
}

/// 13 位毫秒 Unix 时间戳（IM/网盘命名）。无时区语义，直接当 UTC。
fn parse_unix_millis(stem: &str) -> Option<Candidate> {
    if stem.len() != 13 || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // 13 个 ASCII 数字 ≤ 10^13 < i64::MAX，手卷求和避免 .parse() 的不可达 Err 分支。
    let mut millis: i64 = 0;
    for b in stem.bytes() {
        millis = millis * 10 + i64::from(b - b'0');
    }
    let utc = DateTime::<Utc>::UNIX_EPOCH + TimeDelta::milliseconds(millis);
    Some(Candidate {
        utc,
        offset: None,
        source: Source::FilenameUnixMillis,
        inferred_offset: false,
    })
}

fn naive_to_candidate(
    naive: NaiveDateTime,
    default_offset: FixedOffset,
    source: Source,
) -> Candidate {
    // local = utc + offset → utc = local - offset
    let offset_secs = i64::from(default_offset.local_minus_utc());
    let utc_naive = naive - TimeDelta::seconds(offset_secs);
    let utc = Utc.from_utc_datetime(&utc_naive);
    Candidate {
        utc,
        offset: Some(default_offset),
        source,
        inferred_offset: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn east8() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn img_prefix_phone_pattern_parsed() {
        // spec §2.P2 手机命名
        let c = parse_filename("IMG_20240501_143000.jpg", east8()).unwrap();
        assert_eq!(c.source, Source::FilenamePhone);
        assert_eq!(c.offset, Some(east8()));
        assert!(c.inferred_offset);
        // 本地 14:30 +08:00 = UTC 06:30
        assert_eq!(c.utc.timestamp(), 1_714_545_000);
    }

    #[test]
    fn dsc_prefix_camera_pattern_parsed() {
        let c = parse_filename("DSC_20240501_143000.jpg", utc()).unwrap();
        assert_eq!(c.source, Source::FilenameCamera);
        assert_eq!(c.utc.timestamp(), 1_714_573_800);
    }

    #[test]
    fn screenshot_pattern_parsed() {
        let c = parse_filename("Screenshot_2024-05-17-12-00-00.jpg", utc()).unwrap();
        assert_eq!(c.source, Source::FilenameScreenshot);
        assert_eq!(c.utc.timestamp(), 1_715_947_200);
    }

    #[test]
    fn unix_millis_13_digits_parsed() {
        let c = parse_filename("1715961600000.jpg", utc()).unwrap();
        assert_eq!(c.source, Source::FilenameUnixMillis);
        assert_eq!(c.offset, None);
        assert!(!c.inferred_offset);
        assert_eq!(c.utc.timestamp(), 1_715_961_600);
    }

    #[test]
    fn no_extension_uses_full_name_as_stem() {
        // 13 位无扩展名仍可解析
        let c = parse_filename("1715961600000", utc()).unwrap();
        assert_eq!(c.source, Source::FilenameUnixMillis);
    }

    #[test]
    fn img_wrong_length_returns_none() {
        // 长度不足 15
        assert!(parse_filename("IMG_2024050_143000.jpg", utc()).is_none());
    }

    #[test]
    fn img_invalid_date_value_returns_none() {
        // 13 月不合法
        assert!(parse_filename("IMG_20241332_143000.jpg", utc()).is_none());
    }

    #[test]
    fn screenshot_wrong_length_returns_none() {
        assert!(parse_filename("Screenshot_2024-05-17.jpg", utc()).is_none());
    }

    #[test]
    fn screenshot_invalid_value_returns_none() {
        assert!(parse_filename("Screenshot_2024-13-32-25-99-99.jpg", utc()).is_none());
    }

    #[test]
    fn unix_millis_wrong_length_returns_none() {
        // 12 位 → 不匹配 13 位规则
        assert!(parse_filename("171596160000.jpg", utc()).is_none());
    }

    #[test]
    fn unix_millis_non_digit_returns_none() {
        // 13 字符但含字母 → 不匹配
        assert!(parse_filename("171596160000a.jpg", utc()).is_none());
    }

    #[test]
    fn no_known_pattern_returns_none() {
        assert!(parse_filename("random.jpg", utc()).is_none());
    }

    #[test]
    fn east8_local_offset_applied_to_camera() {
        let c = parse_filename("DSC_20240501_143000.jpg", east8()).unwrap();
        // 本地 14:30 +08:00 = UTC 06:30
        let expected = Utc.with_ymd_and_hms(2024, 5, 1, 6, 30, 0).unwrap();
        assert_eq!(c.utc, expected);
    }
}
