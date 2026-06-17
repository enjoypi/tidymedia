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
mod tests {
    use super::*;

    fn east8() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn exif_with_all_three_fields_produces_three_candidates() {
        let exif = Exif::with_mime("image/jpeg").with_date_time_original(1_700_000_100);
        let cands = candidates_from_exif(&exif, utc());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, Source::ExifDateTimeOriginal);
    }

    #[test]
    fn exif_with_no_fields_produces_empty() {
        let exif = Exif::with_mime("image/jpeg");
        assert!(candidates_from_exif(&exif, utc()).is_empty());
    }

    #[test]
    fn filename_candidate_extracted_from_path() {
        let p = camino::Utf8Path::new("/tmp/IMG_20240501_143000.jpg");
        let cs = candidates_from_filename(p, east8());
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].source, Source::FilenamePhone);
    }

    #[test]
    fn filename_no_match_returns_empty() {
        let p = camino::Utf8Path::new("/tmp/random.jpg");
        assert!(candidates_from_filename(p, east8()).is_empty());
    }

    #[test]
    fn empty_path_filename_returns_empty() {
        // Utf8Path::new("") 的 file_name() 返回 None
        let p = camino::Utf8Path::new("");
        assert!(candidates_from_filename(p, east8()).is_empty());
    }

    #[test]
    fn push_epoch_zero_skipped() {
        let mut v = Vec::new();
        push_epoch(&mut v, 0, Source::ExifDateTimeOriginal, None, false);
        assert!(v.is_empty());
    }

    #[test]
    fn push_epoch_non_zero_added() {
        let mut v = Vec::new();
        push_epoch(&mut v, 100, Source::ExifDateTimeOriginal, None, false);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].utc.timestamp(), 100);
    }

    /// 覆盖 `i64::try_from(secs).ok()?` Err arm：`u64::MAX` 超 `i64::MAX` → None。
    #[test]
    fn epoch_to_candidate_u64_above_i64_max_returns_none() {
        assert!(epoch_to_candidate(u64::MAX, Source::ExifDateTimeOriginal, None, false).is_none());
    }

    /// 覆盖 `TimeDelta::try_seconds(signed)?` Err arm：`i64::MAX` 通过 `try_from` 但
    /// 超 `TimeDelta` 内部 secs*1000 上限 → None。
    #[test]
    fn epoch_to_candidate_overflows_timedelta_returns_none() {
        let secs = u64::try_from(i64::MAX).unwrap();
        assert!(epoch_to_candidate(secs, Source::ExifDateTimeOriginal, None, false).is_none());
    }

    /// 覆盖 `UNIX_EPOCH.checked_add_signed(delta)?` Err arm：constructs delta that
    /// passes `try_seconds` but `UNIX_EPOCH` + delta exceeds `DateTime` year range。
    #[test]
    fn epoch_to_candidate_exceeds_datetime_range_returns_none() {
        // TimeDelta MAX secs ≈ i64::MAX/1000 ≈ 9.22e15；DateTime max secs from
        // UNIX_EPOCH ≈ 8.21e15（year ~262143）。落在 (8.21e15, 9.22e15) 区间
        // 即触发 checked_add_signed 返 None。
        let secs: u64 = 8_500_000_000_000_000;
        assert!(epoch_to_candidate(secs, Source::ExifDateTimeOriginal, None, false).is_none());
    }

    /// MKV MIME → `qt_create_date` 候选用 `Source::MkvDateUtc`，offset=None，inferred=false。
    #[test]
    fn mkv_mime_produces_mkv_date_utc_source() {
        let exif = Exif::with_mime("video/x-matroska").with_qt_create_date(1_686_825_000);
        let cands = candidates_from_exif(&exif, utc());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, Source::MkvDateUtc);
        assert_eq!(cands[0].offset, None);
        assert!(!cands[0].inferred_offset);
    }

    /// `video/webm` MIME → 同 MKV 路径，用 `Source::MkvDateUtc`。
    #[test]
    fn webm_mime_produces_mkv_date_utc_source() {
        let exif = Exif::with_mime("video/webm").with_qt_create_date(1_686_825_000);
        let cands = candidates_from_exif(&exif, utc());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, Source::MkvDateUtc);
    }

    /// MP4 MIME → `qt_create_date` 用 `Source::QuickTimeCreationDate`（P0）。
    #[test]
    fn mp4_mime_produces_quicktime_creation_date_source() {
        let exif = Exif::with_mime("video/mp4").with_qt_create_date(1_700_000_100);
        let cands = candidates_from_exif(&exif, utc());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, Source::QuickTimeCreationDate);
        assert_eq!(cands[0].offset, Some(utc()));
        assert!(cands[0].inferred_offset);
    }

    /// 办公文档 `doc_created` → `Source::DocumentCreated`（P0），offset=None，
    /// inferred=false（与 `MkvDateUtc` 同口径，UTC 已归一无需推断）。
    #[test]
    fn doc_created_field_produces_document_created_source() {
        let exif = Exif::with_mime("application/pdf").with_doc_created(1_700_000_200);
        let cands = candidates_from_exif(&exif, utc());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, Source::DocumentCreated);
        assert_eq!(cands[0].offset, None);
        assert!(!cands[0].inferred_offset);
        assert_eq!(cands[0].utc.timestamp(), 1_700_000_200);
    }
}
