//! RIFF AVI 容器内嵌 EXIF 解析。
//!
//! nom-exif 不支持 RIFF 容器，老式相机（Fujifilm `FinePix` 系列等）AVI 的
//! 拍摄时间藏在 `LIST hdrl > LIST strl > strd` chunk：`AVIF` 魔数 + 4 字节
//! 保留区 + 裸 TIFF IFD（小端、无 `II*\0` 头，offset 基准 = 魔数后 4 字节处，
//! 即 `strd` 数据起点 + 8）。IFD 字节解析复用 [`super::tiff_ifd`]
//! （与 PNG `eXIf` chunk / JPEG APP1 fallback 共享）。

use std::io;

use super::backend::MediaReader;
use super::tiff_ifd;

const FOURCC_RIFF: &[u8; 4] = b"RIFF";
const FOURCC_AVI: &[u8; 4] = b"AVI ";
const FOURCC_LIST: &[u8; 4] = b"LIST";
const FOURCC_HDRL: &[u8; 4] = b"hdrl";
const FOURCC_STRL: &[u8; 4] = b"strl";
const FOURCC_STRD: &[u8; 4] = b"strd";
const AVIF_MAGIC: &[u8; 4] = b"AVIF";

/// `strd` 内 IFD offset 的基准：`AVIF` 魔数(4) + 保留区(4) 之后。
const IFD_BASE: usize = 8;

/// hdrl 仅含头信息（真实文件 ~8 KiB）；cap 防损坏 size 字段吃内存。
const MAX_HDRL_BYTES: usize = 1 << 20;
/// hdrl 之前最多容忍的顶层 chunk 数；规范上 hdrl 是首个 LIST。
const MAX_TOP_CHUNKS: usize = 16;
/// LIST 递归深度上限；正常结构 hdrl>strl 仅 1 层，防恶意嵌套爆栈。
const MAX_LIST_DEPTH: u8 = 4;

/// AVI `strd` 内嵌 EXIF 的归档相关字段；schema 与 `tiff_ifd::TiffIfd` 一致，
/// 直接复用即可（日期为 EXIF ASCII 原文，epoch 转换由调用方做）。
pub(crate) type AviExif = tiff_ifd::TiffIfd;

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

// `strd` 数据解析：AVIF 魔数校验后把 `strd[IFD_BASE..]` 当裸 LE IFD0 入口交给
// 公共 `tiff_ifd` 模块（与 PNG `eXIf` chunk / JPEG APP1 fallback 同口径）。
fn parse_avif_ifd(strd: &[u8]) -> Option<AviExif> {
    if strd.get(..4)? != AVIF_MAGIC {
        return None;
    }
    let base = strd.get(IFD_BASE..)?;
    tiff_ifd::parse_ifds(base, 0, tiff_ifd::ByteOrder::Le)
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
