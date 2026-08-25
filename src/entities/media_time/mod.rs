// docs/media-time-detection.md 的方法论实现：
//   P0/P1 候选来自 EXIF + 视频容器（exif::Exif 已经在上层解析好对应字段）
//   P2 候选来自文件名（filename::parse_filename）
//   P3 候选来自 sidecar——协议解析在 `adapters::sidecar` Gateway（XMP/Takeout 是外部
//     数据格式，不属 entities）；entities 只消费转好的 [`Candidate`]
//   P4 候选来自文件系统 mtime（fs_time::from_modified）
// 调用方组装好 Candidate 列表后交给 resolve::resolve 合并 + 冲突校验。

pub mod candidate;
pub mod decision;
pub mod filename;
pub mod filter;
pub mod fs_time;
pub mod priority;
pub mod resolve;

pub use candidate::Candidate;
pub use decision::Confidence;
pub use decision::Conflict;
pub use decision::ConflictKind;
pub use decision::MediaTimeDecision;
pub use priority::Priority;
pub use priority::Source;
pub use resolve::resolve;

use camino::Utf8Path;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::TimeDelta;
use chrono::Utc;

use super::exif::Exif;

/// 把 Exif（已解析的 EXIF/视频容器字段）转成 P0/P1 候选列表。
/// `inferred_offset` 由调用方语义决定：本入口不读 `OffsetTime` 标签，只接受外部 offset。
///
/// 视频容器区分：MKV/WebM 的 `DateUTC` 标 `Source::MkvDateUtc`（P0）；
/// QuickTime/MP4/MOV 的 `creationdate` 标 `Source::QuickTimeCreationDate`（P0）。
///
/// 仅 crate 内部使用——Exif 是 pub(crate) 类型，集成测试请用 `epoch_to_candidate`
/// 直接构造或经由 `filename::parse_filename` / `adapters::sidecar::discover` 等公开入口。
pub(crate) fn candidates_from_exif(exif: &Exif, default_offset: FixedOffset) -> Vec<Candidate> {
    // MKV/WebM 的 DateUTC 是纯 UTC（无时区推断），offset 设 None、inferred=false；
    // QuickTime/MP4 可能含时区（iPhone com.apple.quicktime.creationdate），
    // 或 mvhd 1904-epoch（nom-exif 转成 FixedOffset UTC），均传 default_offset 作推断。
    let (video_source, video_offset, video_inferred) = if exif.is_mkv_container() {
        (Source::MkvDateUtc, None, false)
    } else {
        (Source::QuickTimeCreationDate, Some(default_offset), true)
    };

    let mut out = Vec::new();
    push_epoch(
        &mut out,
        exif.date_time_original(),
        Source::ExifDateTimeOriginal,
        Some(default_offset),
        true,
    );
    push_epoch(
        &mut out,
        exif.qt_create_date(),
        video_source,
        video_offset,
        video_inferred,
    );
    push_epoch(
        &mut out,
        exif.exif_create_date(),
        Source::ExifCreateDate,
        Some(default_offset),
        true,
    );
    // 办公文档容器内创建时间已归一为 Unix UTC epoch，无需 offset 推断；offset=None
    // + inferred_offset=false 与 MkvDateUtc 同口径，让 decision 不当作 naive 解释。
    push_epoch(
        &mut out,
        exif.doc_created(),
        Source::DocumentCreated,
        None,
        false,
    );
    out
}

/// 从路径反推文件名（不依赖 fs 调用），解析 P2 候选。
#[must_use]
pub fn candidates_from_filename(path: &Utf8Path, default_offset: FixedOffset) -> Vec<Candidate> {
    let Some(name) = path.file_name() else {
        return Vec::new();
    };
    filename::parse_filename(name, default_offset)
        .map(|c| vec![c])
        .unwrap_or_default()
}

fn push_epoch(
    out: &mut Vec<Candidate>,
    secs: u64,
    source: Source,
    offset: Option<FixedOffset>,
    inferred_offset: bool,
) {
    if let Some(c) = epoch_to_candidate(secs, source, offset, inferred_offset) {
        out.push(c);
    }
}

/// 把 epoch 秒值转成 Candidate；secs == 0 时认为字段未填，返回 None。
/// 集成测试可借此构造任意来源/优先级的 P0/P1/P3/P4 候选，无需触达 Exif 内部类型。
///
/// 损坏 EXIF（如 u64 字段被填 `0xFFFF_FFFF_FFFF_FFFF`）的 secs 可能超过 `i64::MAX`；
/// 直接 `cast_signed` 会折回大负值绕过 1904/future filter 让文件落 1969/12 桶。
/// 用 `try_from + try_seconds + checked_add_signed` 三段守护，任一溢出即返 None
/// 让 Index 回退到下一优先级候选。
#[must_use]
pub fn epoch_to_candidate(
    secs: u64,
    source: Source,
    offset: Option<FixedOffset>,
    inferred_offset: bool,
) -> Option<Candidate> {
    if secs == 0 {
        return None;
    }
    let signed = i64::try_from(secs).ok()?;
    let delta = TimeDelta::try_seconds(signed)?;
    let utc = DateTime::<Utc>::UNIX_EPOCH.checked_add_signed(delta)?;
    Some(Candidate {
        utc,
        offset,
        source,
        inferred_offset,
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
