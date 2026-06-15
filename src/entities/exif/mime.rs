use std::io;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;

pub(super) const META_TYPE_IMAGE: &str = "image/";
pub(super) const META_TYPE_VIDEO: &str = "video/";
/// RIFF AVI 容器；nom-exif 不支持，走 `entities::riff` 自解析内嵌 EXIF。
pub(super) const MIME_AVI: &str = "video/x-msvideo";
const MIME_QUICKTIME: &str = "video/quicktime";
/// BDAV MPEG-TS（AVCHD .mts / .m2ts）：4 字节 `TP_extra_header` + 188 字节 TS packet。
/// nom-exif 不支持，时间走 P2 文件名启发或 P4 mtime 兜底；本常量只为 `is_media` 通过 MIME 嗅探门槛。
pub(super) const MIME_M2TS: &str = "video/m2ts";
/// 3GPP 手机视频（BMFF `ftyp` brand `3gp*`，常伪装 `.mp4` 扩展名）。
/// `infer` 0.19 的 MP4 matcher 不认 `3gp*` brand；容器本身是 BMFF，
/// 泛 video 路径交 nom-exif 解析 `mvhd.creation_time` 即可。
const MIME_3GPP: &str = "video/3gpp";

/// MIME sniff 时读取的字节数。`infer` 实际只看前 16-32 字节，256 留点余量
/// 让边界 case（容器嵌套）的判定更稳。
const MIME_SNIFF_BYTES: usize = 256;

/// 读首 [`MIME_SNIFF_BYTES`] 字节交给 `infer::get` 推断 MIME；之后 seek 回起点。
/// 调用方需保证 reader 一开始已位于 0；这里 seek(0) 仅作"完成消费后还原"的保险。
///
/// 内部 read / seek 的 `?` Err 分支在 `LocalBackend` 下不可稳定触发，整体标 coverage(off)
/// 沿用 `file_info` 旧 path-only 哈希函数的策略；Backend 调度逻辑由 [`super::Exif::open`] 单测兜底。
pub(super) fn sniff_mime(reader: &mut dyn MediaReader) -> io::Result<String> {
    let mut buf = [0u8; MIME_SNIFF_BYTES];
    let filled = read_fill(reader, &mut buf)?;
    reader.seek(io::SeekFrom::Start(0))?;
    let head = &buf[..filled];
    Ok(infer::get(head)
        .map(|t| t.mime_type().to_string())
        .or_else(|| quicktime_legacy_mime(head).map(str::to_string))
        .or_else(|| m2ts_legacy_mime(head).map(str::to_string))
        .or_else(|| bmff_3gpp_mime(head).map(str::to_string))
        .unwrap_or_default())
}

// `infer` 只匹配 `ftyp` brand 的现代 QuickTime/MP4；老 QuickTime 有两种变体：
//   - `pnot` preview atom 起头（NIKON COOLPIX S5/P5000、Casio 早期机型）
//   - `mdat` 直接起头（无任何头 atom、moov 在文件末尾的 mdat-first 变体）
// 两种 nom-exif 入口 `parse_bmff_mime` 都拒识（只认 ftyp/wide），上游 fork patch
// 在 nom-exif 内部短路；但 sniff_mime 是路由 gate——MIME 为空则 from_reader 不会
// 调 populate_video_dates，fork 永远拿不到执行机会。两种 first-atom 必须都识别
// 为 video/quicktime，否则整段老 QuickTime MOV 被 `is_media` 当作非媒体 ignore。
pub(super) fn quicktime_legacy_mime(buf: &[u8]) -> Option<&'static str> {
    let head = buf.get(4..8)?;
    (head == b"pnot" || head == b"mdat").then_some(MIME_QUICKTIME)
}

// BDAV MPEG-TS（AVCHD .mts / .m2ts）：4 字节 TP_extra_header + 188 字节 TS packet。
// 单 0x47 sync 太弱（H264 SEI / 任意二进制都可能命中），要求 192 byte 间隔连续两个
// sync 才认。`infer` 0.19 不支持 m2ts；不识别会让 is_media=false 致整段 AVCHD 被 ignore。
pub(super) fn m2ts_legacy_mime(buf: &[u8]) -> Option<&'static str> {
    (buf.get(4) == Some(&0x47) && buf.get(196) == Some(&0x47)).then_some(MIME_M2TS)
}

// 标准 BMFF `ftyp` 但 brand 是 `3gp4`/`3gp5` 等：`infer` 0.19 的 MP4 matcher
// 不认 `3gp*` brand，不识别会让 is_media=false 致整段 3GP 手机视频被 ignore。
pub(super) fn bmff_3gpp_mime(buf: &[u8]) -> Option<&'static str> {
    (buf.get(4..8) == Some(b"ftyp") && buf.get(8..11) == Some(b"3gp")).then_some(MIME_3GPP)
}
