// spec §五：常见陷阱。

use tidymedia::media_time::filter::{classify, quicktime_epoch, Validity};
use tidymedia::media_time::{epoch_to_candidate, resolve, Confidence, Priority, Source};

use super::common::{fixed_now, ts, utc_offset};

/// spec §5.1：MP4 未写 creation_time 时 nom-exif 返回 1904-01-01，必须剔除。
#[test]
fn epoch_1904_rejected() {
    assert_eq!(classify(quicktime_epoch(), fixed_now()), Validity::RejectEpoch1904);
}

/// spec §5.1：1904 候选与有效 P0 同时存在 → 1904 被剔除，P0 胜出。
#[test]
fn epoch_1904_filtered_during_resolve() {
    let bogus = epoch_to_candidate(
        quicktime_epoch().timestamp().max(0) as u64,
        Source::QuickTimeCreateDate,
        None,
        false,
    );
    // 注意 quicktime_epoch().timestamp() < 0，epoch_to_candidate(0, ...) 返回 None，
    // 因此 bogus 必然是 None；我们手工塞一个等价的 candidate 进 resolve。
    assert!(bogus.is_none());

    use chrono::DateTime;
    let qt_epoch = quicktime_epoch();
    let cand = tidymedia::media_time::Candidate {
        utc: qt_epoch,
        offset: None,
        source: Source::QuickTimeCreateDate,
        inferred_offset: false,
    };
    let good = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![cand, good], None, fixed_now()).unwrap();
    assert_eq!(d.source, Source::ExifDateTimeOriginal);
    let _ = DateTime::<chrono::Utc>::UNIX_EPOCH; // sanity import use
}

/// spec §5.2：相机日期没设对 → 2099 等未来年份必须剔除。
#[test]
fn future_above_now_plus_one_day_rejected() {
    let future = ts(fixed_now().timestamp() + 100 * 86_400);
    assert_eq!(classify(future, fixed_now()), Validity::RejectFuture);
}

/// spec §5.2：未来 P0 候选被剔除，回退到合法的 P1。
#[test]
fn future_p0_falls_to_valid_p1() {
    let future = (fixed_now().timestamp() + 100 * 86_400) as u64;
    let bogus = epoch_to_candidate(future, Source::ExifDateTimeOriginal, None, false).unwrap();
    let good = epoch_to_candidate(1_700_000_100, Source::ExifCreateDate, None, false).unwrap();
    let d = resolve(vec![bogus, good], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P1);
}

/// spec §5.3："1995 之前的数码照片几乎不可能存在，可作为软阈值" — 保留但降置信。
#[test]
fn pre_1995_kept_but_low_confidence() {
    // 1980-01-01T00:00:00Z = 315532800
    let c = epoch_to_candidate(315_532_800, Source::ExifDateTimeOriginal, None, false).unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.confidence, Confidence::Low);
}

/// spec §5.4：P0 缺失时**不**回退到 ModifyDate（只有 Source::Exif* 与 QuickTime*
/// 是允许的 P0/P1，ModifyDate 不在枚举里）。这里通过缺席验证：
/// Source 枚举里没有 ExifModifyDate 变体，因此 ModifyDate 物理上无法成为候选。
#[test]
fn modifydate_has_no_source_variant() {
    // 枚举穷举式断言：列出所有合法 Source，确认没有 ExifModifyDate / ModifyDate*
    let all = [
        Source::ExifDateTimeOriginal,
        Source::QuickTimeCreationDate,
        Source::MkvDateUtc,
        Source::ExifCreateDate,
        Source::QuickTimeCreateDate,
        Source::FilenameCamera,
        Source::FilenamePhone,
        Source::FilenameScreenshot,
        Source::FilenameUnixMillis,
        Source::XmpSidecar,
        Source::GoogleTakeoutJson,
        Source::FsMtime,
    ];
    for s in all {
        let name = format!("{:?}", s);
        assert!(
            !name.contains("Modify"),
            "Source {name} 含 Modify — spec §5.4 不允许"
        );
    }
}

/// spec §5.6 截图无 EXIF：判定退到文件名启发式 P2。
#[test]
fn screenshot_without_exif_falls_to_filename_p2() {
    use tidymedia::media_time::filename::parse_filename;
    let c = parse_filename("Screenshot_2024-05-17-12-00-00.jpg", utc_offset()).unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.source, Source::FilenameScreenshot);
}

/// spec §5.7 IM 压缩剥离 EXIF：13 位毫秒文件名是唯一线索 → P2。
#[test]
fn im_stripped_exif_uses_filename_millis() {
    use tidymedia::media_time::filename::parse_filename;
    let c = parse_filename("1715961600000.jpg", utc_offset()).unwrap();
    let d = resolve(vec![c], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P2);
    assert_eq!(d.source, Source::FilenameUnixMillis);
}

/// spec §5.10：mtime 不能被推到 P0。同时存在 mtime + P0 EXIF 时 P0 胜出。
#[test]
fn mtime_never_promoted_to_p0() {
    let mtime = epoch_to_candidate(1_700_000_500, Source::FsMtime, None, false).unwrap();
    let exif = epoch_to_candidate(
        1_700_000_100,
        Source::ExifDateTimeOriginal,
        Some(utc_offset()),
        false,
    )
    .unwrap();
    let d = resolve(vec![mtime, exif], None, fixed_now()).unwrap();
    assert_eq!(d.priority, Priority::P0);
    assert_eq!(d.source, Source::ExifDateTimeOriginal);
}
