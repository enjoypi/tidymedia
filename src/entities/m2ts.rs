//! BDAV MPEG-TS（AVCHD .MTS/.M2TS）MDPM 拍摄时间解析。
//!
//! nom-exif 不支持 MPEG-TS 容器；Canon 等 AVCHD 摄像机把拍摄时间嵌在
//! H.264 SEI `user_data_unregistered`（UUID = MDPM）里，exiftool 读作
//! `H264:DateTimeOriginal`。本模块不做 PMT/NAL 级解析（YAGNI）：按
//! AVCHD 规范固定的 video PID 重组 payload 后直接搜 MDPM UUID 字节序列
//! ——PID 重组天然解决 UUID 被 TS 包头切断的问题。
//!
//! MDPM 字节布局（对真实 Canon HF 系列文件逐字节对账 exiftool 验证）：
//! `UUID(16) + "MDPM"(4) + count(1) + count × [tag(1) + data(4)]`，
//! tag `0x18` = `[时区, 世纪 BCD, 年 BCD, 月 BCD]`、
//! tag `0x19` = `[日 BCD, 时 BCD, 分 BCD, 秒 BCD]`。
//! 时区字节忽略：与图片 EXIF / AVI strd 同口径按配置时区解释 naive 时间。

use super::backend::MediaReader;

/// BDAV 物理包：4 字节 `TP_extra_header` + 188 字节标准 TS packet。
const BDAV_PACKET_SIZE: usize = 192;
const TS_SYNC: u8 = 0x47;
/// AVCHD 规范固定的 video stream PID；不解析 PMT（YAGNI，失败走兜底无害）。
const VIDEO_PID: u16 = 0x1011;
/// H.264 SEI `user_data_unregistered` 中 MDPM 的厂商 UUID。
const MDPM_UUID: [u8; 16] = [
    0x17, 0xee, 0x8c, 0x60, 0xf8, 0x4d, 0x11, 0xd9, 0x8c, 0xd6, 0x08, 0x00, 0x20, 0x0c, 0x9a, 0x66,
];
/// UUID 之后的 4 字节标记。
const MDPM_MARKER: &[u8; 4] = b"MDPM";
/// 扫描包数上限（≈96 KiB）。真实文件 MDPM 在第一个 GOP 的 SEI，
/// 实测 UUID 出现在第 4–12 包；上限防无 MDPM 的大文件白读。
const MAX_SCAN_PACKETS: usize = 512;
/// video PID payload 累积上限。实测 UUID 在重组流前 2 KiB；超限仍未集齐
/// 即认定不存在，防损坏流吃内存。
const MAX_VIDEO_BUF: usize = 16 * 1024;
/// UUID 之后参与解析的窗口：count 上限 64 entries（4+1+320 字节）+
/// emulation prevention 膨胀余量。
const MDPM_WINDOW_BYTES: usize = 512;
/// 单 MDPM 的 entry 数上限；防损坏 count 字节越权读取（真实文件 count=7）。
const MAX_MDPM_ENTRIES: usize = 64;
const TAG_RECORD_DATE: u8 = 0x18;
const TAG_RECORD_TIME: u8 = 0x19;

/// 从 BDAV MPEG-TS reader（须位于流起点）提取 MDPM 拍摄时间，返回与 EXIF
/// 同格式的 `"YYYY:MM:DD HH:MM:SS"`（相机本地时间无时区）。
/// 非 BDAV / 无 MDPM / 结构损坏一律返回 None，由调用方回退其他时间来源。
pub(crate) fn parse_m2ts_datetime(r: &mut dyn MediaReader) -> Option<String> {
    let mut pkt = [0u8; BDAV_PACKET_SIZE];
    let mut buf: Vec<u8> = Vec::new();
    let mut uuid_pos: Option<usize> = None;
    for _ in 0..MAX_SCAN_PACKETS {
        if r.read_exact(&mut pkt).is_err() {
            // EOF / 截断：用已收集的数据尽力解析（截断 fixture 即此路径）。
            break;
        }
        let Some(payload) = extract_payload(&pkt) else {
            continue;
        };
        if buf.len() + payload.len() > MAX_VIDEO_BUF {
            break;
        }
        buf.extend_from_slice(payload);
        if uuid_pos.is_none() {
            uuid_pos = buf.windows(MDPM_UUID.len()).position(|w| w == MDPM_UUID);
        }
        // UUID 已命中后继续收包，直到解析窗口集齐再停（UUID 可能落在
        // 当前包尾部，MDPM entries 还在后续包里）。
        if let Some(pos) = uuid_pos
            && buf.len() >= pos + MDPM_UUID.len() + MDPM_WINDOW_BYTES
        {
            break;
        }
    }
    let pos = uuid_pos?;
    // position 保证 UUID 完整在 buf 内 → start ≤ buf.len()，直接索引无 `?` 死区。
    let start = pos + MDPM_UUID.len();
    let end = buf.len().min(start + MDPM_WINDOW_BYTES);
    let cleaned = strip_emulation_prevention(&buf[start..end]);
    parse_mdpm(&cleaned)
}

/// 解出单个 BDAV 包中属于 video PID 的 TS payload。
/// sync 丢失 / 非 video PID / 无 payload / adaptation field 越界均返回 None。
fn extract_payload(pkt: &[u8; BDAV_PACKET_SIZE]) -> Option<&[u8]> {
    // 偏移 0..4 是 TP_extra_header（到达时戳），TS 头从偏移 4 起。
    if pkt[4] != TS_SYNC {
        return None;
    }
    let pid = (u16::from(pkt[5] & 0x1f) << 8) | u16::from(pkt[6]);
    if pid != VIDEO_PID {
        return None;
    }
    let afc = (pkt[7] >> 4) & 0x3;
    if afc & 0x1 == 0 {
        // adaptation field only：无 payload。
        return None;
    }
    let mut off = 8;
    if afc & 0x2 != 0 {
        off += 1 + usize::from(pkt[8]);
    }
    // adaptation field 长度越界时 get 返 None；恰好填满（off == 192）得空切片无害。
    pkt.get(off..)
}

/// H.264 RBSP 解码：移除 emulation prevention bytes（`00 00` 后插入的 `03`）。
/// SEI payload 属于 RBSP，午夜整点（`00:00:00`）的 BCD 时间会触发编码器
/// 插入 EP 字节使 entries 移位，必须先剥离。
fn strip_emulation_prevention(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut zeros = 0_u32;
    for &b in raw {
        if b == 0x03 && zeros >= 2 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

/// 解析 UUID 之后（已剥 EP）的 MDPM 数据：`"MDPM" + count + entries`。
/// 取 Record Date（0x18）+ Record Time（0x19）组合成 EXIF ASCII 格式；
/// 日期合法性（月/日/时分秒范围、闰年）由调用方的 `parse_from_str` 终验。
fn parse_mdpm(data: &[u8]) -> Option<String> {
    if data.get(..4)? != MDPM_MARKER {
        return None;
    }
    let count = usize::from(*data.get(4)?);
    if count == 0 || count > MAX_MDPM_ENTRIES {
        return None;
    }
    let entries = data.get(5..5 + count * 5)?;
    let mut rec_date: Option<&[u8]> = None;
    let mut rec_time: Option<&[u8]> = None;
    for e in entries.chunks_exact(5) {
        match e[0] {
            TAG_RECORD_DATE => rec_date = Some(&e[1..]),
            TAG_RECORD_TIME => rec_time = Some(&e[1..]),
            _ => {}
        }
    }
    let (d, t) = (rec_date?, rec_time?);
    // d[0] 是时区字节，忽略（模块注释有 WHY）。
    let year = bcd(d[1])? * 100 + bcd(d[2])?;
    let month = bcd(d[3])?;
    let day = bcd(t[0])?;
    let hour = bcd(t[1])?;
    let minute = bcd(t[2])?;
    let second = bcd(t[3])?;
    Some(format!(
        "{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}"
    ))
}

/// 两位 BCD 解码；任一 nibble > 9 即损坏数据返回 None。
fn bcd(b: u8) -> Option<u32> {
    let hi = b >> 4;
    let lo = b & 0x0f;
    (hi <= 9 && lo <= 9).then(|| u32::from(hi) * 10 + u32::from(lo))
}

#[cfg(test)]
#[path = "m2ts_tests.rs"]
mod tests;
