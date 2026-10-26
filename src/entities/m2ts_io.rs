use super::super::backend::MediaReader;
use super::{
    BDAV_PACKET_SIZE, MAX_SCAN_PACKETS, MAX_VIDEO_BUF, MDPM_UUID, MDPM_WINDOW_BYTES,
    extract_payload, parse_mdpm, strip_emulation_prevention,
};

pub(crate) fn parse_m2ts_datetime(r: &mut dyn MediaReader) -> Option<String> {
    let mut pkt = [0u8; BDAV_PACKET_SIZE];
    let mut buf: Vec<u8> = Vec::new();
    let mut uuid_pos: Option<usize> = None;
    for _ in 0..MAX_SCAN_PACKETS {
        if r.read_exact(&mut pkt).is_err() {
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
        if let Some(pos) = uuid_pos
            && buf.len() >= pos + MDPM_UUID.len() + MDPM_WINDOW_BYTES
        {
            break;
        }
    }
    let pos = uuid_pos?;
    let start = pos + MDPM_UUID.len();
    let end = buf.len().min(start + MDPM_WINDOW_BYTES);
    let cleaned = strip_emulation_prevention(&buf[start..end]);
    parse_mdpm(&cleaned)
}
