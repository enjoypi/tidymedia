use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use chrono::FixedOffset;
use nom_exif::EntryValue;
use nom_exif::ExifTag;
use nom_exif::MediaParser;
use nom_exif::MediaSource;
use nom_exif::TrackInfoTag;

use super::backend::local::LocalBackend;
use super::backend::{Backend, MediaReader};
use super::common;
use super::uri::Location;

const META_TYPE_IMAGE: &str = "image/";
const META_TYPE_VIDEO: &str = "video/";

/// MIME sniff 时读取的字节数。`infer` 实际只看前 16-32 字节，256 留点余量
/// 让边界 case（容器嵌套）的判定更稳。
const MIME_SNIFF_BYTES: usize = 256;

/// 容器内自带时间字段。文件系统 mtime / btime 不在此结构体里——
/// Clean Architecture 的边界让 Exif 只持有 EXIF/视频容器数据；
/// 文件系统时间由 `entities::media_time::fs_time` 直接从 fs::Metadata 取。
/// spec §5.4：EXIF ModifyDate 故意不解析，避免编辑/导出时间污染判定。
#[derive(Clone, Debug, Default)]
pub struct Exif {
    mime_type: String,

    exif_create_date: u64,
    date_time_original: u64,

    // 视频容器（QuickTime / MP4 / MKV）创建时间。
    // iPhone 的 `com.apple.quicktime.creationdate`（带时区）被 nom-exif
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
        let backend = LocalBackend::arc();
        Self::open(&Location::Local(path.to_path_buf()), &backend, local_offset)
    }

    /// Backend Gateway 入口：从 [`Location`] 用 backend 打开 reader，
    /// sniff_mime 在原 reader 上 seek(0) 之后把句柄交给 [`Self::from_reader`] 解析。
    /// 单次 open_read 减少远端 backend 的往返次数。
    pub fn open(
        loc: &Location,
        backend: &Arc<dyn Backend>,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let mut reader = backend.open_read(loc)?;
        let mime_type = sniff_mime(reader.as_mut())?;
        Self::from_reader(reader, &mime_type, local_offset)
    }

    /// 用调用方已 sniff 好的 MIME + 已 seek 到起点的 reader 解析容器内时间。
    /// 不再触碰 IO 入口，便于 fake backend 单测各种 MIME 分支。
    pub fn from_reader(
        reader: Box<dyn MediaReader>,
        mime_type: &str,
        local_offset: FixedOffset,
    ) -> common::Result<Self> {
        let mut exif = Exif {
            mime_type: mime_type.to_string(),
            ..Default::default()
        };
        if mime_type.starts_with(META_TYPE_IMAGE) {
            populate_image_dates(reader, &mut exif, local_offset);
        } else if mime_type.starts_with(META_TYPE_VIDEO) {
            populate_video_dates(reader, &mut exif, local_offset);
        }
        Ok(exif)
    }

    pub fn mime_type(&self) -> &str {
        self.mime_type.as_str()
    }

    pub fn exif_create_date(&self) -> u64 {
        self.exif_create_date
    }

    pub fn date_time_original(&self) -> u64 {
        self.date_time_original
    }

    pub fn qt_create_date(&self) -> u64 {
        self.qt_create_date
    }

    pub fn is_media(&self) -> bool {
        let mime_type = self.mime_type();
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !mime_type.ends_with(".fpx")
    }
}

/// 读首 [`MIME_SNIFF_BYTES`] 字节交给 `infer::get` 推断 MIME；之后 seek 回起点。
/// 调用方需保证 reader 一开始已位于 0；这里 seek(0) 仅作"完成消费后还原"的保险。
///
/// 内部 read / seek 的 `?` Err 分支在 LocalBackend 下不可稳定触发，整体标 coverage(off)
/// 沿用 file_info 旧 path-only 哈希函数的策略；Backend 调度逻辑由 [`Exif::open`] 单测兜底。
#[cfg_attr(coverage_nightly, coverage(off))]
fn sniff_mime(reader: &mut dyn MediaReader) -> io::Result<String> {
    let mut buf = [0u8; MIME_SNIFF_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
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

fn populate_image_dates(
    reader: Box<dyn MediaReader>,
    exif: &mut Exif,
    local_offset: FixedOffset,
) {
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
        exif.exif_create_date = entry_value_to_epoch(v, local_offset);
    }
    // spec §5.4：ExifTag::ModifyDate 故意不读，避免被编辑/导出时间污染判定。
}

// parse_track 内部 Err 需要构造"header 通过 sniff 但容器结构损坏"的特殊视频 fixture，
// 实务里不可稳定触发；整体标 coverage(off) 与 populate_image_dates 用 nom-exif 库的
// 失败路径同源（image 路径下 PNG without EXIF 走 Some/None 分支已天然覆盖）。
#[cfg_attr(coverage_nightly, coverage(off))]
fn populate_video_dates(
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
