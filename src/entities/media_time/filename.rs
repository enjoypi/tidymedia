// 文件名启发式。支持多类常见模板；未匹配返回 None。
// 文件名提取的时间通常无时区，调用方传入的 default_offset 当本地时区参与解释，
// 并把 `inferred_offset` 标为 true。
//
// 匹配器集合在 `filename_matchers`（各 `try_*` 纯函数）；本文件保留入口编排、
// 常量与共享 helper（`stem_without_ext` / `naive_to_candidate` / `millis_str_to_candidate`）。
#[path = "filename_matchers.rs"]
mod matchers;

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
// macOS 截图：`Screen Shot YYYY-MM-DD at HH.MM.SS[ (N)].png`（注意有空格）。
const MACOS_SCREENSHOT_PREFIX: &str = "Screen Shot ";
// 微信导出：mmexport<13-digit-ms>.jpg
const MMEXPORT_PREFIX: &str = "mmexport";
// QQ 导出：QQ图片<14-digit YYYYMMDDHHMMSS>.jpg
pub(super) const QQ_EXPORT_PREFIX: &str = "QQ图片";
// WhatsApp: "WhatsApp Image YYYY-MM-DD at HH.MM.SS" / "WhatsApp Video …"
const WHATSAPP_IMAGE_PREFIX: &str = "WhatsApp Image ";
const WHATSAPP_VIDEO_PREFIX: &str = "WhatsApp Video ";
// `<前缀>YYYY-MM-DD at HH.MM.SS` 通用模板宽度（WhatsApp / macOS 截图共用）。
pub(super) const AT_DOTTED_LEN: usize = 22;

/// 解析 `path.file_name()`（不含目录），匹配则返回 P2 候选。
#[must_use]
pub fn parse_filename(name: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let stem = stem_without_ext(name);
    if let Some(c) = matchers::try_camera_or_phone(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_pixel(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_screenshot(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_macos_screenshot(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_mmexport(stem) {
        return Some(c);
    }
    if let Some(c) = matchers::try_whatsapp(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_bare_yyyymmdd(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_unix_millis(stem) {
        return Some(c);
    }
    if let Some(c) = matchers::try_generic_dashed(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_parenthesized_compact(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_qq_export(stem, default_offset) {
        return Some(c);
    }
    if let Some(c) = matchers::try_loose_yyyymmdd(stem, default_offset) {
        return Some(c);
    }
    None
}

fn stem_without_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
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

pub(super) fn naive_to_candidate(
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
