#[path = "m2ts_io.rs"]
mod m2ts_io;

pub(crate) use self::m2ts_io::parse_m2ts_datetime;

const BDAV_PACKET_SIZE: usize = 192;
const TS_SYNC: u8 = 0x47;
const VIDEO_PID: u16 = 0x1011;
const MDPM_UUID: [u8; 16] = [
    0x17, 0xee, 0x8c, 0x60, 0xf8, 0x4d, 0x11, 0xd9, 0x8c, 0xd6, 0x08, 0x00, 0x20, 0x0c, 0x9a, 0x66,
];
const MDPM_MARKER: &[u8; 4] = b"MDPM";
const MAX_SCAN_PACKETS: usize = 512;
const MAX_VIDEO_BUF: usize = 16 * 1024;
const MDPM_WINDOW_BYTES: usize = 512;
const MAX_MDPM_ENTRIES: usize = 64;
const TAG_RECORD_DATE: u8 = 0x18;
const TAG_RECORD_TIME: u8 = 0x19;

fn extract_payload(pkt: &[u8; BDAV_PACKET_SIZE]) -> Option<&[u8]> {
    if pkt[4] != TS_SYNC {
        return None;
    }
    let pid = (u16::from(pkt[5] & 0x1f) << 8) | u16::from(pkt[6]);
    if pid != VIDEO_PID {
        return None;
    }
    let afc = (pkt[7] >> 4) & 0x3;
    if afc & 0x1 == 0 {
        return None;
    }
    let mut off = 8;
    if afc & 0x2 != 0 {
        off += 1 + usize::from(pkt[8]);
    }
    pkt.get(off..)
}

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
    for e in entries.as_chunks::<5>().0 {
        match e[0] {
            TAG_RECORD_DATE => rec_date = Some(&e[1..]),
            TAG_RECORD_TIME => rec_time = Some(&e[1..]),
            _ => {}
        }
    }
    let (d, t) = (rec_date?, rec_time?);
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

fn bcd(b: u8) -> Option<u32> {
    let hi = b >> 4;
    let lo = b & 0x0f;
    if hi <= 9 && lo <= 9 {
        Some(u32::from(hi) * 10 + u32::from(lo))
    } else {
        None
    }
}

#[cfg(test)]
#[path = "m2ts_tests.rs"]
mod tests;
