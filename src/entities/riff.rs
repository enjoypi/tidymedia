//! RIFF AVI 容器内嵌 EXIF 解析。
//!
//! nom-exif 不支持 RIFF 容器，老式相机（Fujifilm `FinePix` 系列等）AVI 的
//! 拍摄时间藏在 `LIST hdrl > LIST strl > strd` chunk：`AVIF` 魔数 + 4 字节
//! 保留区 + 裸 TIFF IFD（小端、无 `II*\0` 头，offset 基准 = 魔数后 4 字节处，
//! 即 `strd` 数据起点 + 8）。本模块只读取归档需要的 4 个 ASCII/LONG 标签，
//! 不实现完整 TIFF 解析（YAGNI）。

use std::io;

use super::backend::MediaReader;

const FOURCC_RIFF: &[u8; 4] = b"RIFF";
const FOURCC_AVI: &[u8; 4] = b"AVI ";
const FOURCC_LIST: &[u8; 4] = b"LIST";
const FOURCC_HDRL: &[u8; 4] = b"hdrl";
const FOURCC_STRL: &[u8; 4] = b"strl";
const FOURCC_STRD: &[u8; 4] = b"strd";
const AVIF_MAGIC: &[u8; 4] = b"AVIF";

/// `strd` 内 IFD offset 的基准：`AVIF` 魔数(4) + 保留区(4) 之后。
const IFD_BASE: usize = 8;

// EXIF 标签号与类型（TIFF 6.0 / EXIF 2.31）。
const TAG_MAKE: u16 = 0x010f;
const TAG_MODEL: u16 = 0x0110;
const TAG_EXIF_OFFSET: u16 = 0x8769;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_CREATE_DATE: u16 = 0x9004;
const TYPE_ASCII: u16 = 2;
const TYPE_LONG: u16 = 4;

/// hdrl 仅含头信息（真实文件 ~8 KiB）；cap 防损坏 size 字段吃内存。
const MAX_HDRL_BYTES: usize = 1 << 20;
/// hdrl 之前最多容忍的顶层 chunk 数；规范上 hdrl 是首个 LIST。
const MAX_TOP_CHUNKS: usize = 16;
/// LIST 递归深度上限；正常结构 hdrl>strl 仅 1 层，防恶意嵌套爆栈。
const MAX_LIST_DEPTH: u8 = 4;
/// 单个 ASCII 字段长度上限；Make/Model/日期实测均 <64 字节。
const MAX_ASCII_BYTES: usize = 256;

/// AVI `strd` 内嵌 EXIF 的归档相关字段；日期为 EXIF ASCII 原文
/// （`"YYYY:MM:DD HH:MM:SS"`，相机本地时间无时区），epoch 转换由调用方做。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AviExif {
    pub date_time_original: Option<String>,
    pub create_date: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
}

/// 从 AVI reader（须位于流起点）提取内嵌 EXIF 字段。
/// 非 AVI / 无 `strd` / 结构损坏一律返回 None，由调用方回退其他时间来源。
pub(crate) fn parse_avi_exif(r: &mut dyn MediaReader) -> Option<AviExif> {
    parse_avif_ifd(&find_strd(r)?)
}

// 顶层扫描：RIFF 头 → 找 LIST hdrl → 整段读入内存交给纯函数扫 strd。
// movi（影像数据，可达数 GiB）按 size 直接 seek 跳过，不入内存。
fn find_strd(r: &mut dyn MediaReader) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 12];
    r.read_exact(&mut hdr).ok()?;
    if &hdr[..4] != FOURCC_RIFF || &hdr[8..] != FOURCC_AVI {
        return None;
    }
    for _ in 0..MAX_TOP_CHUNKS {
        let mut ch = [0u8; 8];
        r.read_exact(&mut ch).ok()?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as usize;
        if &ch[..4] == FOURCC_LIST && size >= 4 {
            let mut list_type = [0u8; 4];
            r.read_exact(&mut list_type).ok()?;
            if &list_type == FOURCC_HDRL {
                let body = size - 4;
                if body > MAX_HDRL_BYTES {
                    return None;
                }
                let mut buf = vec![0u8; body];
                r.read_exact(&mut buf).ok()?;
                return find_strd_in(&buf, 0).map(<[u8]>::to_vec);
            }
            skip(r, size - 4 + (size & 1))?;
        } else {
            skip(r, size + (size & 1))?;
        }
    }
    None
}

fn skip(r: &mut dyn MediaReader, n: usize) -> Option<()> {
    // n 来自 u32 chunk size（≤ 4 GiB），转 i64 永不溢出；unwrap_or 仅安抚类型。
    let n = i64::try_from(n).unwrap_or(i64::MAX);
    r.seek(io::SeekFrom::Current(n)).ok().map(|_| ())
}

// hdrl 内存扫描：LIST strl 递归进入，strd 命中即返回其数据切片。
// chunk size 为奇数时按 RIFF 规范补 1 字节对齐。
fn find_strd_in(buf: &[u8], depth: u8) -> Option<&[u8]> {
    if depth > MAX_LIST_DEPTH {
        return None;
    }
    let mut off = 0;
    // get 成功即保证 chunk header 8 字节在界内；off 越界时自然结束循环
    // （比 `off + 8 <= len` 哨兵少一处永不触发的 size 读取 `?` 死区）。
    while let Some(size_bytes) = buf.get(off + 4..off + 8) {
        let fourcc = &buf[off..off + 4];
        let size = u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]])
            as usize;
        let body = off + 8;
        // 损坏 chunk（list_type 或 body 切片越界）必须跳过本 chunk 继续扫，而非 `?`
        // 让整层 find_strd_in 提前 return None —— 同一 hdrl 可能含多 LIST strl，
        // 某 strl size 字段损坏不该让后续合法 strl 内的 strd 一并丢失。
        if fourcc == FOURCC_STRD {
            return buf.get(body..body + size);
        }
        if fourcc == FOURCC_LIST
            && size >= 4
            && buf.get(body..body + 4).is_some_and(|s| s == FOURCC_STRL)
            && let Some(inner) = buf.get(body + 4..body + size)
            && let Some(hit) = find_strd_in(inner, depth + 1)
        {
            return Some(hit);
        }
        off = body + size + (size & 1);
    }
    None
}

// `strd` 数据解析：AVIF 魔数校验后扫 IFD0（Make/Model + ExifOffset 指针），
// 再扫 ExifIFD（两个日期）。结构性越界返回 None（已填字段随之丢弃——
// 损坏数据不值得部分信任）。
fn parse_avif_ifd(strd: &[u8]) -> Option<AviExif> {
    if strd.get(..4)? != AVIF_MAGIC {
        return None;
    }
    let base = strd.get(IFD_BASE..)?;
    let mut out = AviExif::default();
    let mut exif_ifd: Option<usize> = None;
    scan_ifd(base, 0, &mut out, &mut exif_ifd)?;
    if let Some(off) = exif_ifd {
        // ExifIFD 扫描失败不影响已收集的 Make/Model/DTO；忽略其 Option 返回。
        let _ = scan_ifd(base, off, &mut out, &mut exif_ifd);
    }
    Some(out)
}

// 扫单个 IFD 填充 out；外参 `exif_ifd` 在命中 ExifOffset tag 时被写入。
// 返回 `Option<()>` 表达"IFD 本身可读"——旧实现以 usize::MAX 作"指针缺失"
// 哨兵会在 debug 构建触发 u16le(off=usize::MAX) → off + 2 算术溢出 panic。
fn scan_ifd(
    base: &[u8],
    ifd_off: usize,
    out: &mut AviExif,
    exif_ifd: &mut Option<usize>,
) -> Option<()> {
    let count = u16le(base, ifd_off)? as usize;
    for i in 0..count {
        let e = ifd_off + 2 + i * 12;
        let tag = u16le(base, e)?;
        let typ = u16le(base, e + 2)?;
        let cnt = u32le(base, e + 4)? as usize;
        let val = u32le(base, e + 8)? as usize;
        match (tag, typ) {
            (TAG_EXIF_OFFSET, TYPE_LONG) => *exif_ifd = Some(val),
            (TAG_MAKE, TYPE_ASCII) => out.make = read_ascii(base, val, cnt),
            (TAG_MODEL, TYPE_ASCII) => out.model = read_ascii(base, val, cnt),
            (TAG_DATE_TIME_ORIGINAL, TYPE_ASCII) => {
                out.date_time_original = read_ascii(base, val, cnt);
            }
            (TAG_CREATE_DATE, TYPE_ASCII) => out.create_date = read_ascii(base, val, cnt),
            _ => {}
        }
    }
    Some(())
}

// ASCII 字段读取：目标标签（日期 20 字节、Make/Model >4 字节）均走 offset
// 间接存储；cnt ≤ 4 的内联形式对这些标签不会出现，按损坏数据拒绝。
fn read_ascii(base: &[u8], off: usize, cnt: usize) -> Option<String> {
    if cnt <= 4 || cnt > MAX_ASCII_BYTES {
        return None;
    }
    let raw = base.get(off..off + cnt)?;
    let s = std::str::from_utf8(raw)
        .ok()?
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!s.is_empty()).then_some(s)
}

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[cfg(test)]
#[path = "riff_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "riff_structure_tests.rs"]
mod structure_tests;

#[cfg(test)]
#[path = "riff_ifd_tests.rs"]
mod ifd_tests;

#[cfg(test)]
#[path = "riff_io_tests.rs"]
mod io_tests;
