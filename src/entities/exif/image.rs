use std::io;

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::Utc;
use nom_exif::ExifTag;
use nom_exif::MediaParser;
use nom_exif::MediaSource;
use nom_exif::URational;
use tracing::debug;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;
use super::super::xmp;
use super::types::Exif;
use super::types::entry_value_to_epoch;

/// XMP packet fallback 扫描窗口。单段 APP1 最大 65533 字节，64 KB 覆盖单段
/// XMP packet 起始；ExtendedXMP（跨多 APP1 段）不在范围。
const XMP_SCAN_BYTES: usize = 64 * 1024;

pub(super) fn populate_image_dates(
    mut reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
    // 先 buffer 头部供 XMP fallback；seek 回起点后再喂给 nom-exif。
    // seek 失败时跳过 nom-exif 主路径但仍尝试 XMP fallback——head 已读入，
    // 仅靠头部字节即可补 P0/P1 候选，比 mtime 兜底准确得多。
    let mut head = vec![0u8; XMP_SCAN_BYTES];
    let head_len = read_fill(reader.as_mut(), &mut head).unwrap_or(0);
    head.truncate(head_len);
    if reader.seek(io::SeekFrom::Start(0)).is_err() {
        populate_image_xmp_fallback(&head, exif);
        return;
    }

    let Ok(ms) = MediaSource::seekable(reader) else {
        populate_image_xmp_fallback(&head, exif);
        return;
    };
    let mut parser = MediaParser::new();
    let Ok(iter) = parser.parse_exif(ms) else {
        populate_image_xmp_fallback(&head, exif);
        return;
    };
    let parsed: nom_exif::Exif = iter.into();
    if let Some(v) = parsed.get(ExifTag::DateTimeOriginal) {
        exif.date_time_original = entry_value_to_epoch(v, local_offset);
    }
    if let Some(v) = parsed.get(ExifTag::CreateDate) {
        exif.create_date = entry_value_to_epoch(v, local_offset);
    }
    // GPSDateStamp + GPSTimeStamp 合成 GPS UTC 作校验锚点。
    exif.gps_utc = parse_gps_utc(&parsed);
    // ModifyDate 不进时间候选（编辑/导出时间会污染判定），仅供多数派仲裁
    // 识别 re-save 痕迹（filename+mtime+ModifyDate 三方互证 → 否决推翻 P0）。
    if let Some(v) = parsed.get(ExifTag::ModifyDate) {
        exif.modify_date = entry_value_to_epoch(v, local_offset);
    }
    // Make / Model：仅在 EXIF 存在时读取；用于 archive_template 占位符。
    exif.make = parsed
        .get(ExifTag::Make)
        .and_then(|v| v.as_str().map(str::to_owned));
    exif.model = parsed
        .get(ExifTag::Model)
        .and_then(|v| v.as_str().map(str::to_owned));

    // XMP fallback：EXIF DTO/CreateDate 均缺（re-tag 后 IFD0 仅剩 ModifyDate 类
    // 场景）时扫已 buffer 的头部，从 XMP packet 补 P0/P1 候选。
    if exif.date_time_original == 0 && exif.create_date == 0 {
        populate_image_xmp_fallback(&head, exif);
    }
}

pub(super) fn populate_image_xmp_fallback(head: &[u8], exif: &mut Exif) {
    let Some(packet) = xmp::find_xmp_packet(head) else {
        return;
    };
    let dates = xmp::parse_xmp_dates(packet);
    if let Some(dt) = dates.photoshop_date_created {
        // XMP 时间带 timezone，DateTime<FixedOffset>::timestamp() 直接是 UTC epoch。
        // Exif 字段为 u64 不能存 1970 前的负值（数字摄影几乎不涉及，扫描件 XMP
        // 偶有 1960s 标注会落入此分支）；丢弃前发 debug 让缺口可见。
        let secs = dt.timestamp();
        if secs > 0 {
            exif.date_time_original = secs.cast_unsigned();
        } else {
            debug!(
                feature = "exif",
                operation = "xmp_fallback",
                field = "DateTimeOriginal",
                value = %dt,
                secs,
                "xmp date predates Unix epoch; cannot encode in u64 field"
            );
        }
    }
    if let Some(dt) = dates.xmp_create_date {
        let secs = dt.timestamp();
        if secs > 0 {
            exif.create_date = secs.cast_unsigned();
        } else {
            debug!(
                feature = "exif",
                operation = "xmp_fallback",
                field = "CreateDate",
                value = %dt,
                secs,
                "xmp date predates Unix epoch; cannot encode in u64 field"
            );
        }
    }
}

/// 从已解析的 EXIF 读 `GPSDateStamp`（文本 "YYYY:MM:DD"）和
/// `GPSTimeStamp`（3 元素 `URationalArray`：[时, 分, 秒]），合成 GPS UTC。
/// GPS 时间永远是 UTC。任一字段缺失或格式非法均返回 None。
///
/// nom-exif 把 GPS 子 IFD 条目按 IFD 索引 ≥ 2 存入 `Exif`，无法用 `get()`
/// 直接读；改用 `iter()` 遍历所有 IFD 条目按 tag code 匹配。
// 用 raw tag code 替代 `.tag()`：避免 Unknown(_) None arm（无法稳定构造
// 真 EXIF fixture 含 Unknown tag）。GPS code 由 EXIF spec 固定，nom-exif
// const fn `code()` 可在 const 上下文求值。
const GPS_DATE_STAMP: u16 = ExifTag::GPSDateStamp.code();
const GPS_TIME_STAMP: u16 = ExifTag::GPSTimeStamp.code();

fn parse_gps_utc(parsed: &nom_exif::Exif) -> Option<DateTime<Utc>> {
    let mut date_str: Option<String> = None;
    let mut time_rationals: Option<[URational; 3]> = None;

    for entry in parsed.iter() {
        match entry.tag.code() {
            GPS_DATE_STAMP => {
                date_str = entry.value.as_str().map(str::to_owned);
            }
            GPS_TIME_STAMP => {
                // GPSTimeStamp per EXIF spec 必为 3 元素 URational；nom-exif 解析必返
                // URationalArray(len=3)。None/非 3 元素 arm 不可达，用 .and_then + try_from
                // 折叠两层短路成单表达式，消除 if-let branch counter。
                time_rationals = entry
                    .value
                    .as_urational_slice()
                    .and_then(|s| <[URational; 3]>::try_from(s).ok())
                    .or(time_rationals);
            }
            _ => {}
        }
    }

    build_gps_utc(date_str.as_deref(), time_rationals)
}

/// `date_str` = "YYYY:MM:DD", `time` = [hour, min, sec] Rational。
/// 全部转为整秒（纳秒丢弃），合成 `DateTime<Utc>`。
pub(super) fn build_gps_utc(
    date_str: Option<&str>,
    time: Option<[URational; 3]>,
) -> Option<DateTime<Utc>> {
    let date = parse_gps_date(date_str?)?;
    let [h, m, s] = time?;
    let hour = rational_to_u32(h)?;
    let min = rational_to_u32(m)?;
    let sec = rational_to_u32(s)?;
    date.and_hms_opt(hour, min, sec).map(|ndt| ndt.and_utc())
}

pub(super) fn parse_gps_date(s: &str) -> Option<NaiveDate> {
    // GPSDateStamp 格式 "YYYY:MM:DD"（exiftool/EXIF spec 2.31）
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let y: i32 = parts[0].trim().parse().ok()?;
    let mo: u32 = parts[1].trim().parse().ok()?;
    let d: u32 = parts[2].trim().parse().ok()?;
    NaiveDate::from_ymd_opt(y, mo, d)
}

pub(super) fn rational_to_u32(r: URational) -> Option<u32> {
    let denom = r.denominator();
    if denom == 0 {
        return None;
    }
    Some(r.numerator() / denom)
}
