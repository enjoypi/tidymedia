// 文件名启发式。支持多类常见模板；未匹配返回 None。
// 文件名提取的时间通常无时区，调用方传入的 default_offset 当本地时区参与解释，
// 并把 `inferred_offset` 标为 true。

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
const VIDEO_PHONE_PREFIX: &str = "VID_";
const PIXEL_PREFIX: &str = "PXL_";
const SCREENSHOT_PREFIX: &str = "Screenshot_";
// 微信导出：mmexport<13-digit-ms>.jpg
const MMEXPORT_PREFIX: &str = "mmexport";
// WhatsApp: "WhatsApp Image YYYY-MM-DD at HH.MM.SS" / "WhatsApp Video …"
const WHATSAPP_IMAGE_PREFIX: &str = "WhatsApp Image ";
const WHATSAPP_VIDEO_PREFIX: &str = "WhatsApp Video ";

/// 解析 `path.file_name()`（不含目录），匹配则返回 P2 候选。
#[must_use]
pub fn parse_filename(name: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let stem = stem_without_ext(name);
    if let Some(c) = try_camera_or_phone(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = try_pixel(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = try_screenshot(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = try_mmexport(stem) {
        return Some(c);
    }
    if let Some(c) = try_whatsapp(name, default_offset) {
        return Some(c);
    }
    if let Some(c) = try_bare_yyyymmdd(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = try_unix_millis(stem) {
        return Some(c);
    }
    None
}

fn stem_without_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

fn try_camera_or_phone(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let (rest, source) = if let Some(r) = stem.strip_prefix(PHONE_PREFIX) {
        (r, Source::FilenamePhone)
    } else if let Some(r) = stem.strip_prefix(CAMERA_PREFIX) {
        (r, Source::FilenameCamera)
    } else if let Some(r) = stem.strip_prefix(VIDEO_PHONE_PREFIX) {
        (r, Source::FilenameVideoPhone)
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

/// Google Pixel：`PXL_yyyymmdd_HHMMSSmmm[.MP][.PORTRAIT]…`。
/// 时间部分 = `yyyymmdd_HHMMSS`（前 15 chars），尾部毫秒和后缀丢弃。
fn try_pixel(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let rest = stem.strip_prefix(PIXEL_PREFIX)?;
    // 至少 15 chars（日期+时间），后面可以有毫秒或其他标记
    if rest.len() < 15 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(&rest[..15], "%Y%m%d_%H%M%S").ok()?;
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenamePixel,
    ))
}

fn try_screenshot(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
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

/// 微信导出：`mmexport<13-digit-ms>`；直接当 UTC（无时区语义）。
fn try_mmexport(stem: &str) -> Option<Candidate> {
    let rest = stem.strip_prefix(MMEXPORT_PREFIX)?;
    millis_str_to_candidate(rest, Source::FilenameWeChatExport)
}

/// `WhatsApp`：`WhatsApp {Image|Video} YYYY-MM-DD at HH.MM.SS[ (N)]`（含扩展名）。
/// 时区：`WhatsApp` 写设备本地时间，用 `default_offset` 推断。
fn try_whatsapp(name: &str, default_offset: FixedOffset) -> Option<Candidate> {
    // 先剥扩展名再解析
    let stem = stem_without_ext(name);
    let rest = stem
        .strip_prefix(WHATSAPP_IMAGE_PREFIX)
        .or_else(|| stem.strip_prefix(WHATSAPP_VIDEO_PREFIX))?;
    // rest = "YYYY-MM-DD at HH.MM.SS[ (N)]"，取前 19 chars 为日期时间
    // 格式：yyyy-mm-dd at HH.MM.SS → %Y-%m-%d at %H.%M.%S = 22 chars
    if rest.len() < 22 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(&rest[..22], "%Y-%m-%d at %H.%M.%S").ok()?;
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenameWhatsApp,
    ))
}

/// 裸格式：`YYYYMMDD_HHMMSS`（无前缀，15 chars stem）。
fn try_bare_yyyymmdd(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    if stem.len() != 15 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(stem, "%Y%m%d_%H%M%S").ok()?;
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenameBareYyyymmdd,
    ))
}

/// 纯 13 位毫秒 Unix 时间戳（`IM_/网盘/通用命名`）。无时区语义，直接当 UTC。
/// 长度/纯数字校验由 `millis_str_to_candidate` 单点负责（重复 guard 会产生
/// 等价变异且违反 DRY）。
fn try_unix_millis(stem: &str) -> Option<Candidate> {
    millis_str_to_candidate(stem, Source::FilenameUnixMillis)
}

/// 把 13 位纯数字毫秒字符串转成 UTC Candidate。
/// 手卷累加避免 `.parse::<i64>()` 的不可达 Err region。
fn millis_str_to_candidate(digits: &str, source: Source) -> Option<Candidate> {
    if digits.len() != 13 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut millis: i64 = 0;
    for b in digits.bytes() {
        millis = millis * 10 + i64::from(b - b'0');
    }
    let utc = DateTime::<Utc>::UNIX_EPOCH + TimeDelta::milliseconds(millis);
    Some(Candidate {
        utc,
        offset: None,
        source,
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
#[path = "filename_tests.rs"]
mod tests;
