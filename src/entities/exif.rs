use std::fs;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use camino::Utf8Path;
use chrono::FixedOffset;
use nom_exif::EntryValue;
use nom_exif::ExifTag;
use nom_exif::TrackInfoTag;

use super::common;

const META_TYPE_IMAGE: &str = "image/";
const META_TYPE_VIDEO: &str = "video/";

// 时钟偏移宽容值：未来超过此秒数视为伪造（相机日期没设对、电池掉电跳到 2099 年等）。
// 24h 足够覆盖跨时区上传 + NTP 漂移，又不会让真正未来的伪造时间逃逸。
const FUTURE_SKEW_SECS: u64 = 86_400;

#[derive(Clone, Debug, Default)]
pub struct Exif {
    mime_type: String,

    file_modify_date: u64,
    file_create_date: u64,

    exif_create_date: u64,
    exif_modify_date: u64,
    date_time_original: u64,

    // 视频容器（QuickTime / MP4 / MKV）创建时间。
    // 注：iPhone 的 `com.apple.quicktime.creationdate`（带时区）被 nom-exif
    // 内部合并到 TrackInfoTag::CreateDate，因此这里只读一个字段即可。
    qt_create_date: u64,
}

impl Exif {
    /// EXIF DateTimeOriginal/CreateDate/ModifyDate 标准定义为相机本地时间、无时区。
    /// 若 EXIF 内同时含 OffsetTimeOriginal 标签，nom-exif 自动合并为带时区的
    /// `EntryValue::DateTime`，本入口的 offset 对其无影响；否则落入 NaiveDateTime
    /// 分支时，调用方传入的 offset 当作相机本地时区参与 epoch 转换。
    pub fn from_path_with_offset(
        path: &Utf8Path,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let meta = fs::metadata(path)?;

        let mime_type = infer::get_from_path(path)?
            .map(|t| t.mime_type().to_string())
            .unwrap_or_default();

        let mut exif = Exif {
            mime_type,
            file_modify_date: system_time_to_epoch(meta.modified().ok()),
            file_create_date: system_time_to_epoch(meta.created().ok()),
            ..Default::default()
        };

        if exif.mime_type.starts_with(META_TYPE_IMAGE) {
            populate_image_dates(path, &mut exif, local_offset);
        } else if exif.mime_type.starts_with(META_TYPE_VIDEO) {
            populate_video_dates(path, &mut exif, local_offset);
        }

        Ok(exif)
    }

    pub fn mime_type(&self) -> &str {
        self.mime_type.as_str()
    }

    pub fn file_modify_date(&self) -> u64 {
        self.file_modify_date
    }

    pub fn file_create_date(&self) -> u64 {
        self.file_create_date
    }

    pub fn exif_create_date(&self) -> u64 {
        self.exif_create_date
    }

    pub fn exif_modify_date(&self) -> u64 {
        self.exif_modify_date
    }

    pub fn date_time_original(&self) -> u64 {
        self.date_time_original
    }

    pub fn qt_create_date(&self) -> u64 {
        self.qt_create_date
    }

    /// 按"拍摄 → 容器 → 文件 → ModifyDate"顺序回退到第一个非伪造时间。
    /// exif_modify_date 放在 file 时间之后：ModifyDate 常被 Lightroom/Photoshop
    /// 改写为编辑/导出时间，比文件 mtime/btime 更不可信。
    pub fn media_create_date(&self) -> u64 {
        if !self.is_media() {
            return 0;
        }
        let upper = future_skew_cap();
        let pick = |t: u64| t > 0 && t < upper;

        if pick(self.date_time_original()) {
            return self.date_time_original();
        }
        if pick(self.qt_create_date()) {
            return self.qt_create_date();
        }
        if pick(self.exif_create_date()) {
            return self.exif_create_date();
        }

        if self.file_modify_date() > self.file_create_date() && pick(self.file_create_date()) {
            return self.file_create_date();
        }
        if pick(self.file_modify_date()) {
            return self.file_modify_date();
        }
        if pick(self.file_create_date()) {
            return self.file_create_date();
        }

        if pick(self.exif_modify_date()) {
            return self.exif_modify_date();
        }

        0
    }

    pub fn is_media(&self) -> bool {
        let mime_type = self.mime_type();
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !mime_type.ends_with(".fpx")
    }
}

fn system_time_to_epoch(t: Option<SystemTime>) -> u64 {
    t.and_then(|s| s.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// SystemTime::now() 在常规系统上不会 Err（要早于 UNIX_EPOCH 才会）；
// 极端情况下退到 u64::MAX 等价"无上限"，保留兼容行为而非崩溃。
fn future_skew_cap() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().saturating_add(FUTURE_SKEW_SECS))
        .unwrap_or(u64::MAX)
}

fn entry_value_to_epoch(v: &EntryValue, local_offset: FixedOffset) -> u64 {
    let secs = match v {
        // 带时区：nom-exif 已经合成 DateTime<FixedOffset>，timestamp() 直接是 UTC epoch。
        EntryValue::DateTime(dt) => dt.timestamp(),
        // 无时区：相机/编码器没写 OffsetTime；按调用方注入的本地时区解释。
        EntryValue::NaiveDateTime(nd) => nd
            .and_local_timezone(local_offset)
            .single()
            .map(|x| x.timestamp())
            .unwrap_or(0),
        _ => return 0,
    };
    if secs <= 0 {
        0
    } else {
        secs as u64
    }
}

fn populate_image_dates(path: &Utf8Path, exif: &mut Exif, local_offset: FixedOffset) {
    let Ok(parsed) = nom_exif::read_exif(path.as_str()) else {
        return;
    };
    if let Some(v) = parsed.get(ExifTag::DateTimeOriginal) {
        exif.date_time_original = entry_value_to_epoch(v, local_offset);
    }
    if let Some(v) = parsed.get(ExifTag::CreateDate) {
        exif.exif_create_date = entry_value_to_epoch(v, local_offset);
    }
    if let Some(v) = parsed.get(ExifTag::ModifyDate) {
        exif.exif_modify_date = entry_value_to_epoch(v, local_offset);
    }
}

fn populate_video_dates(path: &Utf8Path, exif: &mut Exif, local_offset: FixedOffset) {
    let Ok(track) = nom_exif::read_track(path.as_str()) else {
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

    /// 跨模块测试用：根据 MIME 构造一个除 mime_type 外全部为 0 的 Exif。
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
}

#[cfg(test)]
#[path = "exif_tests.rs"]
mod tests;
