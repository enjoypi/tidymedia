use chrono::FixedOffset;
use nom_exif::MediaParser;
use nom_exif::MediaSource;
use nom_exif::TrackInfoTag;

use super::super::backend::MediaReader;
use super::types::Exif;
use super::types::entry_value_to_epoch;

// AVI（RIFF）路径：`entities::riff` 提取 strd 内嵌 EXIF 的 ASCII 字段后在此转
// epoch。日期与图片 EXIF 同语义（相机本地时间无时区），按调用方 offset 解释；
// Make/Model 一并填充供 archive_template `{make}/{model}` 使用。
pub(super) fn populate_avi_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Some(avi) = crate::entities::riff::parse_avi_exif(reader.as_mut()) else {
        return;
    };
    exif.date_time_original = avi
        .date_time_original
        .as_deref()
        .map_or(0, |s| ascii_datetime_to_epoch(s, local_offset));
    exif.create_date = avi
        .create_date
        .as_deref()
        .map_or(0, |s| ascii_datetime_to_epoch(s, local_offset));
    exif.make = avi.make;
    exif.model = avi.model;
}

// M2TS（BDAV MPEG-TS）路径：`entities::m2ts` 提取 H.264 SEI MDPM 拍摄时间
// 后在此转 epoch。MDPM 只有单一拍摄时刻，仅填 P0（date_time_original），
// 不伪造 P1；时区口径与图片 EXIF / AVI 一致（naive + 配置时区）。
pub(super) fn populate_m2ts_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Some(dt) = crate::entities::m2ts::parse_m2ts_datetime(reader.as_mut()) else {
        return;
    };
    exif.date_time_original = ascii_datetime_to_epoch(&dt, local_offset);
}

// EXIF ASCII 日期（"YYYY:MM:DD HH:MM:SS"）转 epoch；非法格式 / 1970 前返回 0
// （与 entry_value_to_epoch 的"0 = 字段未填"约定一致）。
pub(super) fn ascii_datetime_to_epoch(s: &str, local_offset: FixedOffset) -> u64 {
    chrono::NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .ok()
        .and_then(|nd| nd.and_local_timezone(local_offset).single())
        .map_or(0, |dt| {
            let secs = dt.timestamp();
            if secs <= 0 { 0 } else { secs.cast_unsigned() }
        })
}

// parse_track 内部 Err 需要构造"header 通过 sniff 但容器结构损坏"的特殊视频 fixture，
// 实务里不可稳定触发；整体标 coverage(off) 与 populate_image_dates 用 nom-exif 库的
// 失败路径同源（image 路径下 PNG without EXIF 走 Some/None 分支已天然覆盖）。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn populate_video_dates(
    reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    let Ok(ms) = MediaSource::seekable(reader) else {
        return;
    };
    let mut parser = MediaParser::new();
    let Ok(track) = parser.parse_track(ms) else {
        return;
    };
    if let Some(v) = track.get(TrackInfoTag::CreateDate) {
        exif.qt_create_date = entry_value_to_epoch(v, local_offset);
    }
}
