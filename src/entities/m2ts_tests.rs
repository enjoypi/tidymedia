use std::io::Cursor;

use super::*;

// ── 合成 BDAV builder ──────────────────────────────────────────────

/// 192 字节 BDAV 包：`TP_extra`(4) + TS 头(4) + `[af]` + payload，`0xff` 填充。
/// `af` = `Some(n)` 时 AFC=11 且 adaptation field 长度字节为 n。
fn bdav_packet(sync: u8, pid: u16, af: Option<u8>, payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0u8; 4];
    pkt.push(sync);
    pkt.push(u8::try_from(pid >> 8).unwrap() & 0x1f);
    pkt.push(u8::try_from(pid & 0xff).unwrap());
    match af {
        Some(n) => {
            pkt.push(0x30); // AFC=11：adaptation field + payload
            pkt.push(n);
            pkt.extend(std::iter::repeat_n(0u8, usize::from(n)));
        }
        None => pkt.push(0x10), // AFC=01：仅 payload
    }
    pkt.extend_from_slice(payload);
    assert!(pkt.len() <= BDAV_PACKET_SIZE, "payload too large");
    pkt.resize(BDAV_PACKET_SIZE, 0xff);
    pkt
}

fn video_packet(payload: &[u8]) -> Vec<u8> {
    bdav_packet(TS_SYNC, VIDEO_PID, None, payload)
}

/// `UUID + "MDPM" + count=2 + [0x18 date] + [0x19 time]`。
fn mdpm_block(date: [u8; 4], time: [u8; 4]) -> Vec<u8> {
    let mut b = MDPM_UUID.to_vec();
    b.extend_from_slice(MDPM_MARKER);
    b.push(2);
    b.push(TAG_RECORD_DATE);
    b.extend_from_slice(&date);
    b.push(TAG_RECORD_TIME);
    b.extend_from_slice(&time);
    b
}

/// 真实 Canon 字节：2011-10-01 10:35:57（tz=0x10 忽略）。
fn canon_block() -> Vec<u8> {
    mdpm_block([0x10, 0x20, 0x11, 0x10], [0x01, 0x10, 0x35, 0x57])
}

fn parse(stream: &[u8]) -> Option<String> {
    parse_m2ts_datetime(&mut Cursor::new(stream.to_vec()))
}

// ── 流级解析 ──────────────────────────────────────────────────────

#[test]
fn real_fixture_extracts_mdpm_datetime() {
    let mut f = std::fs::File::open("tests/data/sample-canon-avchd.m2ts").unwrap();
    assert_eq!(
        parse_m2ts_datetime(&mut f).as_deref(),
        Some("2011:10:01 10:35:57")
    );
}

#[test]
fn synthetic_stream_extracts_datetime() {
    assert_eq!(
        parse(&video_packet(&canon_block())).as_deref(),
        Some("2011:10:01 10:35:57")
    );
}

#[test]
fn uuid_spanning_two_packets_found() {
    // 第一包 payload 填满 184 字节、UUID 前 8 字节压轴；后半与 entries 在
    // 第二包开头——重组后字节连续，UUID 才可被 windows 搜到。
    let block = canon_block();
    let (head, tail) = block.split_at(8);
    let mut first = vec![0xee; 184 - head.len()];
    first.extend_from_slice(head);
    let mut stream = video_packet(&first);
    stream.extend(video_packet(tail));
    assert_eq!(parse(&stream).as_deref(), Some("2011:10:01 10:35:57"));
}

#[test]
fn empty_reader_returns_none() {
    assert_eq!(parse(&[]), None);
}

#[test]
fn truncated_packet_returns_none() {
    assert_eq!(parse(&[0u8; 100]), None);
}

#[test]
fn bad_sync_packet_skipped_then_found() {
    let mut stream = bdav_packet(0x00, VIDEO_PID, None, &canon_block());
    stream.extend(video_packet(&canon_block()));
    assert_eq!(parse(&stream).as_deref(), Some("2011:10:01 10:35:57"));
}

#[test]
fn wrong_pid_returns_none() {
    assert_eq!(
        parse(&bdav_packet(TS_SYNC, 0x0100, None, &canon_block())),
        None
    );
}

#[test]
fn adaptation_only_packet_has_no_payload() {
    // AFC=10：手工置 AFC 位为 only-adaptation。
    let mut pkt = video_packet(&canon_block());
    pkt[7] = 0x20;
    assert_eq!(parse(&pkt), None);
}

#[test]
fn adaptation_field_before_payload_extracted() {
    let pkt = bdav_packet(TS_SYNC, VIDEO_PID, Some(7), &canon_block());
    assert_eq!(parse(&pkt).as_deref(), Some("2011:10:01 10:35:57"));
}

#[test]
fn adaptation_field_overflow_returns_none() {
    assert_eq!(
        parse(&bdav_packet(
            TS_SYNC,
            VIDEO_PID,
            Some(180),
            &canon_block()[..3]
        )),
        None
    );
}

#[test]
fn video_buf_overflow_returns_none() {
    // 无 UUID 的 video payload 灌满 MAX_VIDEO_BUF 触发提前放弃。
    let pkt = video_packet(&[0xaa; 184]);
    let need = MAX_VIDEO_BUF / 184 + 2;
    let stream: Vec<u8> = pkt.iter().copied().cycle().take(need * 192).collect();
    assert_eq!(parse(&stream), None);
}

#[test]
fn scan_limit_exhausted_returns_none() {
    // 全部非 video PID：循环空转到 MAX_SCAN_PACKETS 上限。
    let pkt = bdav_packet(TS_SYNC, 0x0100, None, &[0xaa; 32]);
    let stream: Vec<u8> = pkt
        .iter()
        .copied()
        .cycle()
        .take((MAX_SCAN_PACKETS + 4) * 192)
        .collect();
    assert_eq!(parse(&stream), None);
}

#[test]
fn window_completes_across_trailing_packets() {
    // UUID+entries 后跟超过窗口长度的后续 video 包：集齐窗口即停且解析正确。
    let mut stream = video_packet(&canon_block());
    for _ in 0..4 {
        stream.extend(video_packet(&[0xbb; 184]));
    }
    assert_eq!(parse(&stream).as_deref(), Some("2011:10:01 10:35:57"));
}

#[test]
fn midnight_emulation_prevention_stripped() {
    // 午夜 00:00:00 的 time data 含 00 00 00，RBSP 编码插入 EP 字节 03。
    let mut block = MDPM_UUID.to_vec();
    block.extend_from_slice(MDPM_MARKER);
    block.push(2);
    block.push(TAG_RECORD_DATE);
    block.extend_from_slice(&[0x10, 0x20, 0x11, 0x10]);
    block.push(TAG_RECORD_TIME);
    block.extend_from_slice(&[0x01, 0x00, 0x00, 0x03, 0x00]); // 00 00 03 00 = EP 编码后
    assert_eq!(
        parse(&video_packet(&block)).as_deref(),
        Some("2011:10:01 00:00:00")
    );
}

// ── extract_payload ───────────────────────────────────────────────

fn as_pkt(v: &[u8]) -> &[u8; BDAV_PACKET_SIZE] {
    v.try_into().unwrap()
}

#[test]
fn extract_payload_af_fills_packet_yields_empty_slice() {
    // adaptation field 恰好填满（af_len=183 → off=192）：空 payload 而非越界。
    let pkt = bdav_packet(TS_SYNC, VIDEO_PID, Some(183), &[]);
    assert_eq!(extract_payload(as_pkt(&pkt)), Some(&[][..]));
}

#[test]
fn extract_payload_af_one_past_end_returns_none() {
    let mut pkt = bdav_packet(TS_SYNC, VIDEO_PID, Some(183), &[]);
    pkt[8] = 184; // off=193 > 192
    assert_eq!(extract_payload(as_pkt(&pkt)), None);
}

// ── parse_mdpm（已剥 EP 的 UUID 后数据）────────────────────────────

fn entries(count: u8, body: &[u8]) -> Vec<u8> {
    let mut d = MDPM_MARKER.to_vec();
    d.push(count);
    d.extend_from_slice(body);
    d
}

#[test]
fn parse_mdpm_wrong_marker_returns_none() {
    assert_eq!(parse_mdpm(b"XXXX\x01\x18\x10\x20\x11\x10"), None);
}

#[test]
fn parse_mdpm_short_data_returns_none() {
    assert_eq!(parse_mdpm(b"MD"), None);
}

#[test]
fn parse_mdpm_missing_count_returns_none() {
    assert_eq!(parse_mdpm(b"MDPM"), None);
}

#[test]
fn parse_mdpm_zero_count_returns_none() {
    assert_eq!(parse_mdpm(&entries(0, &[])), None);
}

#[test]
fn parse_mdpm_excessive_count_returns_none() {
    let body = [0u8; 65 * 5];
    assert_eq!(parse_mdpm(&entries(65, &body)), None);
}

#[test]
fn parse_mdpm_truncated_entries_returns_none() {
    // count=2 但只有 1 个 entry 的字节数。
    assert_eq!(
        parse_mdpm(&entries(2, &[0x18, 0x10, 0x20, 0x11, 0x10])),
        None
    );
}

#[test]
fn parse_mdpm_missing_time_tag_returns_none() {
    assert_eq!(
        parse_mdpm(&entries(1, &[0x18, 0x10, 0x20, 0x11, 0x10])),
        None
    );
}

#[test]
fn parse_mdpm_missing_date_tag_returns_none() {
    assert_eq!(
        parse_mdpm(&entries(1, &[0x19, 0x01, 0x10, 0x35, 0x57])),
        None
    );
}

#[test]
fn parse_mdpm_invalid_bcd_in_any_field_returns_none() {
    // 7 个 BCD 位置（世纪/年/月 + 日/时/分/秒）逐一注入 0xff：
    // 每个位置的 `?` None 传播分支都必须拒绝损坏数据。
    fn body(date: [u8; 4], time: [u8; 4]) -> Vec<u8> {
        let mut b = vec![0x18];
        b.extend_from_slice(&date);
        b.push(0x19);
        b.extend_from_slice(&time);
        b
    }
    let date = [0x10, 0x20, 0x11, 0x10];
    let time = [0x01, 0x10, 0x35, 0x57];
    for i in 1..4 {
        let mut d = date;
        d[i] = 0xff;
        assert_eq!(parse_mdpm(&entries(2, &body(d, time))), None, "date[{i}]");
    }
    for i in 0..4 {
        let mut t = time;
        t[i] = 0xff;
        assert_eq!(parse_mdpm(&entries(2, &body(date, t))), None, "time[{i}]");
    }
}

#[test]
fn parse_mdpm_year_1900s_century_decoded() {
    let body = [
        0x18, 0x10, 0x19, 0x99, 0x12, // 1999-12
        0x19, 0x31, 0x23, 0x59, 0x59,
    ];
    assert_eq!(
        parse_mdpm(&entries(2, &body)).as_deref(),
        Some("1999:12:31 23:59:59")
    );
}

#[test]
fn parse_mdpm_unknown_tags_ignored() {
    let body = [
        0x70, 0xca, 0xf2, 0xff, 0xff, // 实测文件中的非时间 tag
        0x18, 0x10, 0x20, 0x11, 0x02, 0x19, 0x10, 0x16, 0x35, 0x45,
    ];
    assert_eq!(
        parse_mdpm(&entries(3, &body)).as_deref(),
        Some("2011:02:10 16:35:45")
    );
}

// ── strip_emulation_prevention / bcd ─────────────────────────────

#[test]
fn strip_ep_removes_03_after_two_zeros() {
    assert_eq!(
        strip_emulation_prevention(&[0x00, 0x00, 0x03, 0x00]),
        vec![0x00, 0x00, 0x00]
    );
}

#[test]
fn strip_ep_keeps_03_after_single_zero() {
    assert_eq!(
        strip_emulation_prevention(&[0x00, 0x03, 0x01]),
        vec![0x00, 0x03, 0x01]
    );
}

#[test]
fn strip_ep_resets_zero_run_after_strip() {
    // 00 00 03 03：第一个 03 是 EP 剥掉，第二个 03 是数据保留。
    assert_eq!(
        strip_emulation_prevention(&[0x00, 0x00, 0x03, 0x03]),
        vec![0x00, 0x00, 0x03]
    );
}

#[test]
fn strip_ep_handles_long_zero_run() {
    // 00 00 03 00 00 03：两处 EP 均剥。
    assert_eq!(
        strip_emulation_prevention(&[0x00, 0x00, 0x03, 0x00, 0x00, 0x03]),
        vec![0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn bcd_decodes_two_digits() {
    assert_eq!(bcd(0x59), Some(59));
}

#[test]
fn bcd_rejects_high_nibble_over_nine() {
    assert_eq!(bcd(0xa1), None);
}

#[test]
fn bcd_rejects_low_nibble_over_nine() {
    assert_eq!(bcd(0x1f), None);
}
