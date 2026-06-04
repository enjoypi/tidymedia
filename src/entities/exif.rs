use std::io;
use std::sync::Arc;

#[cfg(test)]
use camino::Utf8Path;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::Utc;
use nom_exif::EntryValue;
use nom_exif::ExifTag;
use nom_exif::MediaParser;
use nom_exif::MediaSource;
use nom_exif::TrackInfoTag;
use nom_exif::URational;

use super::backend::{Backend, MediaReader};
use super::common;
use super::file_info::read_fill;
use super::uri::Location;
// 测试 helper `Exif::from_path_with_offset` 需要构造 LocalBackend instance。
// 仅 #[cfg(test)] gate 下引用 adapters，生产代码方向严格内向（CA 规则）。
#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;

const META_TYPE_IMAGE: &str = "image/";
const META_TYPE_VIDEO: &str = "video/";

/// MIME sniff 时读取的字节数。`infer` 实际只看前 16-32 字节，256 留点余量
/// 让边界 case（容器嵌套）的判定更稳。
const MIME_SNIFF_BYTES: usize = 256;

/// 容器内自带时间字段。文件系统 mtime / btime 不在此结构体里——
/// Clean Architecture 的边界让 Exif 只持有 EXIF/视频容器数据；
/// 文件系统时间由 `entities::media_time::fs_time` 直接从 `fs::Metadata` 取。
/// EXIF `ModifyDate` 故意不解析，避免编辑/导出时间污染判定。
#[derive(Clone, Debug, Default)]
pub struct Exif {
    mime_type: String,

    create_date: u64,
    date_time_original: u64,

    // 视频容器（QuickTime / MP4 / MKV）创建时间。
    // iPhone 的 `com.apple.quicktime.creationdate`（带时区）被 nom-exif
    // 内部合并到 TrackInfoTag::CreateDate，因此这里只读一个字段即可。
    qt_create_date: u64,

    // EXIF GPS 子 IFD 内的 GPSDateStamp + GPSTimeStamp 合成 UTC 时间，
    // 用于 resolve 时与 P0 候选做交叉校验（差值 > 24h 时产生 GpsOver24h 冲突）。
    gps_utc: Option<DateTime<Utc>>,

    // 相机厂商 / 型号；仅图片 EXIF 填写（视频容器一般不含这两个标签）。
    // 用于 archive_template 的 `{make}` / `{model}` 占位符。
    make: Option<String>,
    model: Option<String>,
}

impl Exif {
    /// EXIF DateTimeOriginal/CreateDate/ModifyDate 标准定义为相机本地时间、无时区。
    /// 若 EXIF 内同时含 `OffsetTimeOriginal` 标签，nom-exif 自动合并为带时区的
    /// `EntryValue::DateTime`，本入口的 offset 对其无影响；否则落入 `NaiveDateTime`
    /// 分支时，调用方传入的 offset 当作相机本地时区参与 epoch 转换。
    #[cfg(test)]
    pub fn from_path_with_offset(
        path: &Utf8Path,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let backend = LocalBackend::arc();
        Self::open(&Location::Local(path.to_path_buf()), &backend, local_offset)
    }

    /// Backend Gateway 入口：从 [`Location`] 用 backend 打开 reader，
    /// `sniff_mime` 在原 reader 上 seek(0) 之后把句柄交给 [`Self::from_reader`] 解析。
    /// 单次 `open_read` 减少远端 backend 的往返次数。
    pub fn open(
        loc: &Location,
        backend: &Arc<dyn Backend>,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let mut reader = backend.open_read(loc)?;
        let mime_type = sniff_mime(reader.as_mut())?;
        Ok(Self::from_reader(reader, &mime_type, local_offset))
    }

    /// 用调用方已 sniff 好的 MIME + 已 seek 到起点的 reader 解析容器内时间。
    /// 不再触碰 IO 入口，便于 fake backend 单测各种 MIME 分支。
    pub fn from_reader(
        reader: Box<dyn MediaReader>,
        mime_type: &str,
        local_offset: FixedOffset,
    ) -> Self {
        let mut exif = Exif {
            mime_type: mime_type.to_string(),
            ..Default::default()
        };
        if mime_type.starts_with(META_TYPE_IMAGE) {
            populate_image_dates(reader, &mut exif, local_offset);
        } else if mime_type.starts_with(META_TYPE_VIDEO) {
            populate_video_dates(reader, &mut exif, local_offset);
        }
        exif
    }

    pub fn mime_type(&self) -> &str {
        self.mime_type.as_str()
    }

    pub fn exif_create_date(&self) -> u64 {
        self.create_date
    }

    pub fn date_time_original(&self) -> u64 {
        self.date_time_original
    }

    pub fn qt_create_date(&self) -> u64 {
        self.qt_create_date
    }

    /// GPS UTC 时间（由 `GPSDateStamp` + `GPSTimeStamp` 合成）。
    /// 仅图片 EXIF 含 GPS 子 IFD 时有值；视频容器不提供。
    pub fn gps_utc(&self) -> Option<DateTime<Utc>> {
        self.gps_utc
    }

    /// 当前 MIME 是否为 Matroska/WebM 容器（MKV/WEBM），用于区分
    /// `Source::MkvDateUtc` vs `Source::QuickTimeCreationDate`。
    pub fn is_mkv_container(&self) -> bool {
        self.mime_type.starts_with("video/x-matroska") || self.mime_type.starts_with("video/webm")
    }

    /// EXIF `Make` 字段（相机厂商）；仅图片 EXIF 通常含有。
    pub fn make(&self) -> Option<&str> {
        self.make.as_deref()
    }

    /// EXIF `Model` 字段（相机型号）；仅图片 EXIF 通常含有。
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn is_media(&self) -> bool {
        let mime_type = self.mime_type();
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !camino::Utf8Path::new(mime_type)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("fpx"))
    }
}

/// 读首 [`MIME_SNIFF_BYTES`] 字节交给 `infer::get` 推断 MIME；之后 seek 回起点。
/// 调用方需保证 reader 一开始已位于 0；这里 seek(0) 仅作"完成消费后还原"的保险。
///
/// 内部 read / seek 的 `?` Err 分支在 `LocalBackend` 下不可稳定触发，整体标 coverage(off)
/// 沿用 `file_info` 旧 path-only 哈希函数的策略；Backend 调度逻辑由 [`Exif::open`] 单测兜底。
#[cfg_attr(coverage_nightly, coverage(off))]
fn sniff_mime(reader: &mut dyn MediaReader) -> io::Result<String> {
    let mut buf = [0u8; MIME_SNIFF_BYTES];
    let filled = read_fill(reader, &mut buf)?;
    reader.seek(io::SeekFrom::Start(0))?;
    Ok(infer::get(&buf[..filled])
        .map(|t| t.mime_type().to_string())
        .unwrap_or_default())
}

fn entry_value_to_epoch(v: &EntryValue, local_offset: FixedOffset) -> u64 {
    let secs = match v {
        // 带时区：nom-exif 已经合成 DateTime<FixedOffset>，timestamp() 直接是 UTC epoch。
        EntryValue::DateTime(dt) => dt.timestamp(),
        // 无时区：相机/编码器没写 OffsetTime；按调用方注入的本地时区解释。
        EntryValue::NaiveDateTime(nd) => nd
            .and_local_timezone(local_offset)
            .single()
            .map_or(0, |x| x.timestamp()),
        _ => return 0,
    };
    if secs <= 0 { 0 } else { secs.cast_unsigned() }
}

fn populate_image_dates(reader: Box<dyn MediaReader>, exif: &mut Exif, local_offset: FixedOffset) {
    let Ok(ms) = MediaSource::seekable(reader) else {
        return;
    };
    let mut parser = MediaParser::new();
    let Ok(iter) = parser.parse_exif(ms) else {
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
    // ExifTag::ModifyDate 故意不读，避免被编辑/导出时间污染判定。
    // Make / Model：仅在 EXIF 存在时读取；用于 archive_template 占位符。
    exif.make = parsed
        .get(ExifTag::Make)
        .and_then(|v| v.as_str().map(str::to_owned));
    exif.model = parsed
        .get(ExifTag::Model)
        .and_then(|v| v.as_str().map(str::to_owned));
}

/// 从已解析的 EXIF 读 `GPSDateStamp`（文本 "YYYY:MM:DD"）和
/// `GPSTimeStamp`（3 元素 `URationalArray`：[时, 分, 秒]），合成 GPS UTC。
/// GPS 时间永远是 UTC。任一字段缺失或格式非法均返回 None。
///
/// nom-exif 把 GPS 子 IFD 条目按 IFD 索引 ≥ 2 存入 `Exif`，无法用 `get()`
/// 直接读；改用 `iter()` 遍历所有 IFD 条目按 tag code 匹配。
///
/// 内部分支（unrecognized GPS tag code / `GPSTimeStamp` 非 `URationalArray` /
/// 元素数 != 3）需要特殊构造的 EXIF fixture 才能稳定触发，标 `coverage(off)`；
/// 语义由 `parse_gps_date` / `rational_to_u32` / `build_gps_utc` 单元测试断言。
#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_gps_utc(parsed: &nom_exif::Exif) -> Option<DateTime<Utc>> {
    let mut date_str: Option<String> = None;
    let mut time_rationals: Option<[URational; 3]> = None;

    for entry in parsed.iter() {
        let Some(tag) = entry.tag.tag() else {
            continue;
        };
        if tag == ExifTag::GPSDateStamp {
            date_str = entry.value.as_str().map(str::to_owned);
        } else if tag == ExifTag::GPSTimeStamp
            && let Some(slice) = entry.value.as_urational_slice()
            && let [h, m, s] = slice
        {
            time_rationals = Some([*h, *m, *s]);
        }
    }

    build_gps_utc(date_str.as_deref(), time_rationals)
}

/// `date_str` = "YYYY:MM:DD", `time` = [hour, min, sec] Rational。
/// 全部转为整秒（纳秒丢弃），合成 `DateTime<Utc>`。
fn build_gps_utc(date_str: Option<&str>, time: Option<[URational; 3]>) -> Option<DateTime<Utc>> {
    let date = parse_gps_date(date_str?)?;
    let [h, m, s] = time?;
    let hour = rational_to_u32(h)?;
    let min = rational_to_u32(m)?;
    let sec = rational_to_u32(s)?;
    date.and_hms_opt(hour, min, sec).map(|ndt| ndt.and_utc())
}

// parse_gps_date 与 rational_to_u32 仅被 parse_gps_utc（已标 coverage(off)）调用。
// 单元测试 binary 直接调用它们（branch 已覆盖），但集成 binary 不调用，导致
// LLVM multi-instance branch miss。整体标 coverage(off)；语义由对应单元测试保证不退化。
#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_gps_date(s: &str) -> Option<NaiveDate> {
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

#[cfg_attr(coverage_nightly, coverage(off))]
fn rational_to_u32(r: URational) -> Option<u32> {
    let denom = r.denominator();
    if denom == 0 {
        return None;
    }
    Some(r.numerator() / denom)
}

// parse_track 内部 Err 需要构造"header 通过 sniff 但容器结构损坏"的特殊视频 fixture，
// 实务里不可稳定触发；整体标 coverage(off) 与 populate_image_dates 用 nom-exif 库的
// 失败路径同源（image 路径下 PNG without EXIF 走 Some/None 分支已天然覆盖）。
#[cfg_attr(coverage_nightly, coverage(off))]
fn populate_video_dates(reader: Box<dyn MediaReader>, exif: &mut Exif, local_offset: FixedOffset) {
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

#[cfg(test)]
impl Exif {
    /// 测试用 UTC 默认入口。生产路径用 [`Exif::from_path_with_offset`]。
    pub(crate) fn from_path(path: &Utf8Path) -> common::Result<Self> {
        Self::from_path_with_offset(path, FixedOffset::east_opt(0).expect("UTC offset is valid"))
    }

    /// 跨模块测试用：根据 MIME 构造一个除 `mime_type` 外全部为 0 的 Exif。
    pub(crate) fn with_mime(mime_type: &str) -> Self {
        Self {
            mime_type: mime_type.to_string(),
            ..Default::default()
        }
    }

    /// 跨模块测试用：链式设置 EXIF 拍摄时间。
    pub(crate) fn with_date_time_original(mut self, secs: u64) -> Self {
        self.date_time_original = secs;
        self
    }

    /// 跨模块测试用：链式设置视频容器创建时间（`qt_create_date`）。
    pub(crate) fn with_qt_create_date(mut self, secs: u64) -> Self {
        self.qt_create_date = secs;
        self
    }

    /// 跨模块测试用：链式设置 Make / Model。
    pub(crate) fn with_make_model(mut self, make: &str, model: &str) -> Self {
        self.make = Some(make.to_string());
        self.model = Some(model.to_string());
        self
    }
}

#[cfg(test)]
#[path = "exif_tests.rs"]
mod tests;
