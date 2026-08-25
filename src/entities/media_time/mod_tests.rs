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
