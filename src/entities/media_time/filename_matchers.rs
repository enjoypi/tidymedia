// 文件名匹配器集合：各 `try_*` 按前缀/形状解析出 P2 候选。
// 与 `parse_filename`（入口编排）分离，纯函数可独立单测。

use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::NaiveDateTime;

use super::{
    AT_DOTTED_LEN, CAMERA_PREFIX, MACOS_SCREENSHOT_PREFIX, MMEXPORT_PREFIX, PHONE_PREFIX,
    PIXEL_PREFIX, SCREENSHOT_PREFIX, VIDEO_PHONE_PREFIX, WHATSAPP_IMAGE_PREFIX,
    WHATSAPP_VIDEO_PREFIX, millis_str_to_candidate, naive_to_candidate,
};
use crate::entities::media_time::candidate::Candidate;
use crate::entities::media_time::priority::Source;

/// 宽松 `YYYYMMDD`：stem 开头或紧跟 `-` / `_` / 空格 之后的 8 位合法日期。
/// 仅日期粒度（时间 00:00:00），比 [`try_bare_yyyymmdd`] 宽松（后者要求严格
/// 15 字符 `YYYYMMDD_HHMMSS`），故放在调用链最末位兜底。年份合理性由
/// `super::filter` 的 `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` 负责。
pub(super) fn try_loose_yyyymmdd(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let bytes = stem.as_bytes();
    let anchors = std::iter::once(0_usize).chain(
        bytes
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| matches!(b, b'-' | b'_' | b' ').then_some(i + 1)),
    );
    for start in anchors {
        let end = start + 8;
        let Some(window) = bytes.get(start..end) else {
            continue;
        };
        if !window.iter().all(u8::is_ascii_digit) {
            continue;
        }
        // 全 ASCII 数字 → str 切片落在 char 边界
        let Ok(date) = NaiveDate::parse_from_str(&stem[start..end], "%Y%m%d") else {
            continue;
        };
        let naive = date
            .and_hms_opt(0, 0, 0)
            .expect("internal: 00:00:00 is always a valid time-of-day");
        return Some(naive_to_candidate(
            naive,
            default_offset,
            Source::FilenameBareYyyymmdd,
        ));
    }
    None
}

/// 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS<任意后缀>`：事后批量重命名工具的常见
/// 格式（相机时钟错误时文件名时间往往才是真实拍摄时间）。最宽松，放最后兜底。
/// 扫描 stem 中第一个形状匹配且日期合法的 19 字节窗口。
pub(super) fn try_generic_dashed(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    const LEN: usize = 19; // "YYYY-MM-DD HH-MM-SS"
    let bytes = stem.as_bytes();
    for i in 0..=bytes.len().checked_sub(LEN)? {
        if !dashed_window_shape_ok(&bytes[i..i + LEN]) {
            continue;
        }
        // 形状匹配的窗口全 ASCII，str 字节切片必然落在 char 边界
        if let Ok(naive) = NaiveDateTime::parse_from_str(&stem[i..i + LEN], "%Y-%m-%d %H-%M-%S") {
            return Some(naive_to_candidate(
                naive,
                default_offset,
                Source::FilenameDashedDateTime,
            ));
        }
    }
    None
}

/// 窗口形状：分隔符位（4,7 为 `-`、10 为空格、13,16 为 `-`），其余位为数字。
fn dashed_window_shape_ok(w: &[u8]) -> bool {
    w.iter().enumerate().all(|(i, &b)| match i {
        4 | 7 | 13 | 16 => b == b'-',
        10 => b == b' ',
        _ => b.is_ascii_digit(),
    })
}

#[path = "filename_matchers_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::{try_parenthesized_compact, try_qq_export};

pub(super) fn try_camera_or_phone(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let (rest, source) = stem
        .strip_prefix(PHONE_PREFIX)
        .map(|r| (r, Source::FilenamePhone))
        .or_else(|| {
            stem.strip_prefix(CAMERA_PREFIX)
                .map(|r| (r, Source::FilenameCamera))
        })
        .or_else(|| {
            stem.strip_prefix(VIDEO_PHONE_PREFIX)
                .map(|r| (r, Source::FilenameVideoPhone))
        })?;
    // 期望格式：yyyymmdd_HHMMSS（8 + 1 + 6 = 15 chars）
    if rest.len() != 15 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(rest, "%Y%m%d_%H%M%S").ok()?;
    Some(naive_to_candidate(naive, default_offset, source))
}

/// Google Pixel：`PXL_yyyymmdd_HHMMSSmmm[.MP][.PORTRAIT]…`。
/// 时间部分 = `yyyymmdd_HHMMSS`（前 15 chars），尾部毫秒和后缀丢弃。
pub(super) fn try_pixel(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
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

pub(super) fn try_screenshot(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let rest = stem.strip_prefix(SCREENSHOT_PREFIX)?;
    // 支持两种主流截图命名（Windows Snip & Sketch / Samsung / MIUI / 原生 Android）：
    //   - yyyy-mm-dd-HH-mm-ss（19 chars，全 dash）
    //   - yyyymmdd_HHMMSS（15 chars，与 IMG_/DSC_ 同模板，Samsung/MIUI 常见）
    // 后者若退到 try_loose_yyyymmdd 兜底会丢失时分秒精度，必须在此显式解析。
    let naive = if rest.len() >= 19
        && let Ok(n) = NaiveDateTime::parse_from_str(&rest[..19], "%Y-%m-%d-%H-%M-%S")
    {
        n
    } else if rest.len() >= 15
        && let Ok(n) = NaiveDateTime::parse_from_str(&rest[..15], "%Y%m%d_%H%M%S")
    {
        n
    } else {
        return None;
    };
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenameScreenshot,
    ))
}

/// 微信导出：`mmexport<13-digit-ms>`；直接当 UTC（无时区语义）。
/// 下载/导出时刻与 mtime 同源，不参与多数派仲裁
/// （[`Source::is_majority_filename_vote`]）。
pub(super) fn try_mmexport(stem: &str) -> Option<Candidate> {
    let rest = stem.strip_prefix(MMEXPORT_PREFIX)?;
    millis_str_to_candidate(rest, Source::FilenameWeChatExport)
}

/// `WhatsApp`：`WhatsApp {Image|Video} YYYY-MM-DD at HH.MM.SS[ (N)]`。
/// 时区：`WhatsApp` 写设备本地时间，用 `default_offset` 推断。
pub(super) fn try_whatsapp(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    try_at_dotted(
        stem,
        WHATSAPP_IMAGE_PREFIX,
        default_offset,
        Source::FilenameWhatsApp,
    )
    .or_else(|| {
        try_at_dotted(
            stem,
            WHATSAPP_VIDEO_PREFIX,
            default_offset,
            Source::FilenameWhatsApp,
        )
    })
}

/// macOS 截图：`Screen Shot YYYY-MM-DD at HH.MM.SS[ (N)].png`（注意 `Screen Shot`
/// 带空格、时间用 `.` 分隔），与 [`try_whatsapp`] 共用 `at HH.MM.SS` 模板。
/// 与 `Screenshot_` 前缀（Android/Windows 截图）互不冲突。
pub(super) fn try_macos_screenshot(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    try_at_dotted(
        stem,
        MACOS_SCREENSHOT_PREFIX,
        default_offset,
        Source::FilenameScreenshot,
    )
}

/// 公共模板 `<prefix>YYYY-MM-DD at HH.MM.SS[尾随后缀]`：剥前缀后取前 22
/// 字节按 `chrono` 解析；尾部 ` (N)` 或扩展名自动忽略。
fn try_at_dotted(
    stem: &str,
    prefix: &str,
    default_offset: FixedOffset,
    source: Source,
) -> Option<Candidate> {
    let rest = stem.strip_prefix(prefix)?;
    if rest.len() < AT_DOTTED_LEN {
        return None;
    }
    let naive =
        NaiveDateTime::parse_from_str(&rest[..AT_DOTTED_LEN], "%Y-%m-%d at %H.%M.%S").ok()?;
    Some(naive_to_candidate(naive, default_offset, source))
}

/// 裸格式：`YYYYMMDD_HHMMSS`（无前缀，15 chars stem）。
pub(super) fn try_bare_yyyymmdd(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
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
/// 等价变异且违反 DRY）。下载时戳与 mtime 同源，不参与多数派仲裁
/// （[`Source::is_majority_filename_vote`]）。
pub(super) fn try_unix_millis(stem: &str) -> Option<Candidate> {
    millis_str_to_candidate(stem, Source::FilenameUnixMillis)
}
