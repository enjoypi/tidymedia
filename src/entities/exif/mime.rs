use std::io;

use super::super::backend::MediaReader;
use super::super::file_info::read_fill;
use super::super::office;

pub(super) const META_TYPE_IMAGE: &str = "image/";
pub(super) const META_TYPE_VIDEO: &str = "video/";
/// PNG 容器；nom-exif 3.6 不解析 `eXIf` chunk，走 `entities::png` 自解析。
pub(super) const MIME_PNG: &str = "image/png";
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

/// 判定 mime 是否属办公文档族（PDF / OOXML / CFB / iWork / ODF / RTF / EPUB /
/// 思维导图 / 纯文本）。`types.rs::from_reader` 用此把命中分流到
/// `populate_document_dates`；新增容器 MUST 同步 `entities/office/mod.rs` MIME 常量。
pub(super) fn is_office_mime(mime: &str) -> bool {
    mime.starts_with(office::MIME_PDF)
        || mime.starts_with(office::MIME_OOXML_PREFIX)
        || mime == office::MIME_DOC
        || mime == office::MIME_PPT
        || mime == office::MIME_XLS
        || mime == office::MIME_PAGES
        || mime == office::MIME_NUMBERS
        || mime == office::MIME_KEYNOTE
        || mime.starts_with(office::MIME_IWORK_PREFIX)
        || mime.starts_with(office::MIME_ODF_PREFIX)
        || mime == office::MIME_RTF_APP
        || mime == office::MIME_RTF_TEXT
        || mime == office::MIME_EPUB
        || mime == office::MIME_XMIND
        || mime == office::MIME_XMIND_ALT
        || mime == office::MIME_FREEMIND
        || mime == office::MIME_MINDNODE
        || mime == office::MIME_ITMZ
        || mime == office::MIME_MINDMANAGER
        || mime == office::MIME_TEXT_PLAIN
        || mime == office::MIME_TEXT_MARKDOWN
        || mime == office::MIME_TEXT_RST
        || mime == office::MIME_TEXT_CSV
        || mime == office::MIME_TEXT_TSV
}

/// 扩展名→MIME 反查。`infer` 不识别纯文本 / RTF / iWork / 思维导图等多数办公自定义 mime，
/// 故 `Exif::open` 在 `sniff_mime` 返空时调用本 fn 兜底。扩展名匹配大小写不敏感。
/// 返 None 表「无对应 mime」，让上层路径保留空 mime（`is_media=false` 兜底）。
#[must_use]
pub(crate) fn mime_from_ext(ext: Option<&str>) -> Option<&'static str> {
    let lower = ext?.to_ascii_lowercase();
    let mime = match lower.as_str() {
        "pdf" => office::MIME_PDF,
        "docx" => office::MIME_DOCX,
        "pptx" => office::MIME_PPTX,
        "xlsx" => office::MIME_XLSX,
        "doc" => office::MIME_DOC,
        "ppt" => office::MIME_PPT,
        "xls" => office::MIME_XLS,
        "pages" => office::MIME_PAGES,
        "numbers" => office::MIME_NUMBERS,
        "key" => office::MIME_KEYNOTE,
        "odt" => office::MIME_ODT,
        "ods" => office::MIME_ODS,
        "odp" => office::MIME_ODP,
        "odg" => office::MIME_ODG,
        "rtf" => office::MIME_RTF_APP,
        "epub" => office::MIME_EPUB,
        "xmind" => office::MIME_XMIND,
        "mm" => office::MIME_FREEMIND,
        "mindnode" => office::MIME_MINDNODE,
        "itmz" => office::MIME_ITMZ,
        "mmap" => office::MIME_MINDMANAGER,
        "txt" | "log" => office::MIME_TEXT_PLAIN,
        "md" | "markdown" => office::MIME_TEXT_MARKDOWN,
        "rst" => office::MIME_TEXT_RST,
        "csv" => office::MIME_TEXT_CSV,
        "tsv" => office::MIME_TEXT_TSV,
        _ => return None,
    };
    Some(mime)
}
