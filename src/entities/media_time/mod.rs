// docs/media-time-detection.md 的方法论实现：
//   P0/P1 候选来自 EXIF + 视频容器（exif::Exif 已经在上层解析好对应字段）
//   P2 候选来自文件名（filename::parse_filename）
//   P3 候选来自 sidecar（sidecar::discover）
//   P4 候选来自文件系统 mtime（fs_time::from_modified）
// 调用方组装好 Candidate 列表后交给 resolve::resolve 合并 + 冲突校验。

pub mod candidate;
pub mod decision;
pub mod filename;
pub mod filter;
pub mod fs_time;
pub mod priority;
pub mod resolve;
pub mod sidecar;

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
/// inferred_offset 由调用方语义决定：本入口不读 OffsetTime 标签，只接受外部 offset。
///
/// 仅 crate 内部使用——Exif 是 pub(crate) 类型，集成测试请用 `epoch_to_candidate`
/// 直接构造或经由 `filename::parse_filename` / `sidecar::discover` 等公开入口。
pub(crate) fn candidates_from_exif(exif: &Exif, default_offset: FixedOffset) -> Vec<Candidate> {
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
        Source::QuickTimeCreationDate,
        Some(default_offset),
        true,
    );
    push_epoch(
        &mut out,
        exif.exif_create_date(),
        Source::ExifCreateDate,
        Some(default_offset),
        true,
    );
    out
}

/// 从路径反推文件名（不依赖 fs 调用），解析 P2 候选。
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
pub fn epoch_to_candidate(
    secs: u64,
    source: Source,
    offset: Option<FixedOffset>,
    inferred_offset: bool,
) -> Option<Candidate> {
    if secs == 0 {
        return None;
    }
    let utc = DateTime::<Utc>::UNIX_EPOCH + TimeDelta::seconds(secs as i64);
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
        let exif = Exif::with_mime("image/jpeg")
            .with_date_time_original(1_700_000_100);
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
}
