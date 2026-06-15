use std::sync::Arc;

#[cfg(test)]
use camino::Utf8Path;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Utc;
use nom_exif::EntryValue;

use super::super::backend::Backend;
use super::super::backend::MediaReader;
use super::super::common;
use super::super::uri::Location;
use super::image::populate_image_dates;
use super::mime::META_TYPE_IMAGE;
use super::mime::META_TYPE_VIDEO;
use super::mime::MIME_AVI;
use super::mime::MIME_M2TS;
use super::mime::sniff_mime;
use super::video::populate_avi_dates;
use super::video::populate_m2ts_dates;
use super::video::populate_video_dates;
#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;

/// 容器内自带时间字段。文件系统 mtime / btime 不在此结构体里——
/// Clean Architecture 的边界让 Exif 只持有 EXIF/视频容器数据；
/// 文件系统时间由 `entities::media_time::fs_time` 直接从 `fs::Metadata` 取。
/// EXIF `ModifyDate` 解析但**不进时间候选**（编辑/导出时间会污染判定），
/// 仅供多数派仲裁识别 re-save 痕迹：filename+mtime 与 `ModifyDate` 三方互证
/// 时说明三者都是 re-save 时戳，不构成推翻 P0 的证据。
#[derive(Clone, Debug, Default)]
pub struct Exif {
    pub(super) mime_type: String,

    pub(super) create_date: u64,
    pub(super) date_time_original: u64,
    pub(super) modify_date: u64,

    // 视频容器（QuickTime / MP4 / MKV）创建时间。
    // iPhone 的 `com.apple.quicktime.creationdate`（带时区）被 nom-exif
    // 内部合并到 TrackInfoTag::CreateDate，因此这里只读一个字段即可。
    pub(super) qt_create_date: u64,

    // EXIF GPS 子 IFD 内的 GPSDateStamp + GPSTimeStamp 合成 UTC 时间，
    // 用于 resolve 时与 P0 候选做交叉校验（差值 > 24h 时产生 GpsOver24h 冲突）。
    pub(super) gps_utc: Option<DateTime<Utc>>,

    // 相机厂商 / 型号；图片 EXIF 与 AVI strd 内嵌 EXIF 填写
    // （QuickTime/MKV 容器一般不含这两个标签）。
    // 用于 archive_template 的 `{make}` / `{model}` 占位符。
    pub(super) make: Option<String>,
    pub(super) model: Option<String>,
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
        } else if mime_type.starts_with(MIME_AVI) {
            // AVI 先于泛 video 分流：nom-exif 不认 RIFF，时间在 strd 内嵌 EXIF。
            populate_avi_dates(reader, &mut exif, local_offset);
        } else if mime_type.starts_with(MIME_M2TS) {
            // M2TS 先于泛 video 分流：nom-exif 不认 MPEG-TS，时间在 H.264 SEI MDPM。
            populate_m2ts_dates(reader, &mut exif, local_offset);
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

    /// EXIF `ModifyDate`（编辑/导出时间）。不进时间候选，仅供多数派仲裁
    /// 识别 re-save 痕迹；0 = 缺失。
    pub fn exif_modify_date(&self) -> u64 {
        self.modify_date
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

pub(super) fn entry_value_to_epoch(v: &EntryValue, local_offset: FixedOffset) -> u64 {
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
