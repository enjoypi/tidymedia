use std::io::{Cursor, Write};

use tidymedia::cfb::{
    append_ascii_runs, append_utf16le_runs, extract_dates, extract_printable_runs, extract_text,
    filetime_to_epoch, find_property_filetime, flush_run, is_text_char, parse, u32_le_at,
};

const FORMAT_ID_SUMMARY: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
const PID_CREATE_DTM: u32 = 0x0C;
const PID_LASTSAVE_DTM: u32 = 0x0D;
const VT_FILETIME: u32 = 0x40;
const FILETIME_TICKS_PER_SEC: u64 = 10_000_000;
const EPOCH_DELTA_SECS: u64 = 11_644_473_600;
fn unix_to_filetime(unix_secs: u64) -> u64 {
    (unix_secs + EPOCH_DELTA_SECS) * FILETIME_TICKS_PER_SEC
}
fn build_summary_propertyset(created_ft: u64, modified_ft: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF]); // ByteOrder LE
    buf.extend_from_slice(&[0, 0]); // Version
    buf.extend_from_slice(&[0, 0, 0, 0]); // SystemId
    buf.extend_from_slice(&[0u8; 16]); // CLSID
    buf.extend_from_slice(&1u32.to_le_bytes()); // NumPropertySets
    buf.extend_from_slice(&FORMAT_ID_SUMMARY); // FMTID
    buf.extend_from_slice(&48u32.to_le_bytes()); // section offset (after header)

    let mut section = Vec::new();
    section.extend_from_slice(&0u32.to_le_bytes()); // section size placeholder
    section.extend_from_slice(&2u32.to_le_bytes()); // num properties
    section.extend_from_slice(&PID_CREATE_DTM.to_le_bytes());
    section.extend_from_slice(&24u32.to_le_bytes()); // prop 1 at offset 24
    section.extend_from_slice(&PID_LASTSAVE_DTM.to_le_bytes());
    section.extend_from_slice(&36u32.to_le_bytes()); // prop 2 at offset 36
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&created_ft.to_le_bytes());
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&modified_ft.to_le_bytes());

    #[expect(
        clippy::cast_possible_truncation,
        reason = "section 长度 < 256，u32 cast 安全"
    )]
    let section_size = section.len() as u32;
    section[0..4].copy_from_slice(&section_size.to_le_bytes());

    buf.extend(section);
    buf
}

#[test]
fn extract_dates_happy_path() {
    let buf = build_summary_propertyset(
        unix_to_filetime(1_487_068_200),
        unix_to_filetime(1_514_808_000),
    );
    assert_eq!(extract_dates(&buf), (1_487_068_200, 1_514_808_000));
}

#[test]
fn extract_dates_too_short_buf_returns_zeros() {
    assert_eq!(extract_dates(b""), (0, 0));
}

#[test]
fn find_property_filetime_pid_not_present() {
    let buf = build_summary_propertyset(0, 0);
    assert!(find_property_filetime(&buf, 0xFFFF).is_none());
}

#[test]
fn find_property_filetime_short_buf_returns_none() {
    assert!(find_property_filetime(&[0u8; 10], PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_byte_order_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[0] = 0x00;
    buf[1] = 0x00;
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_fmtid_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[28] = 0xFF;
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_section_offset_out_of_range_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_section_too_short_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF, 0, 0, 0, 0, 0, 0]);
    buf.extend_from_slice(&[0u8; 16]);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&FORMAT_ID_SUMMARY);
    buf.extend_from_slice(&48u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_num_props_too_large_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[52..56].copy_from_slice(&500u32.to_le_bytes());
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_entries_truncated_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF, 0, 0, 0, 0, 0, 0]);
    buf.extend_from_slice(&[0u8; 16]);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&FORMAT_ID_SUMMARY);
    buf.extend_from_slice(&48u32.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes()); // size
    buf.extend_from_slice(&5u32.to_le_bytes()); // num
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_property_offset_out_of_range_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    let entry_off_in_buf = 48 + 8 + 4;
    buf[entry_off_in_buf..entry_off_in_buf + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn find_property_filetime_wrong_vt_type_returns_none() {
    let mut buf = build_summary_propertyset(0, 0);
    buf[72..76].copy_from_slice(&0x1Eu32.to_le_bytes()); // VT_LPSTR
    assert!(find_property_filetime(&buf, PID_CREATE_DTM).is_none());
}

#[test]
fn filetime_to_epoch_boundaries() {
    assert_eq!(
        filetime_to_epoch(unix_to_filetime(1_487_068_200)),
        Some(1_487_068_200)
    );
    assert!(filetime_to_epoch(0).is_none());
    assert!(filetime_to_epoch(EPOCH_DELTA_SECS * FILETIME_TICKS_PER_SEC).is_none());
}

#[test]
fn u32_le_at_happy() {
    assert_eq!(u32_le_at(&[1, 0, 0, 0], 0), 1);
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn printable_runs_extracts_utf16_chinese() {
    let mut bytes = vec![0xFF_u8, 0x00, 0x01, 0x00];
    bytes.extend(utf16le("增值税发票内容片段"));
    bytes.extend([0x00, 0x00, 0xFF, 0xFE]);
    let mut out = String::new();
    extract_printable_runs(&bytes, &mut out, 256);
    assert!(out.contains("增值税发票内容片段"), "got: {out}");
}

#[test]
fn printable_runs_extracts_long_ascii() {
    let mut out = String::new();
    extract_printable_runs(
        b"\x01\x02this is a plain ascii sentence\x00\x03",
        &mut out,
        256,
    );
    assert!(out.contains("this is a plain ascii sentence"), "got: {out}");
}

#[test]
fn printable_runs_drops_short_fragments() {
    let mut out = String::new();
    extract_printable_runs(b"\x00ab\x00cd\x01ef\x02", &mut out, 256);
    assert_eq!(out, "");
}

#[test]
fn printable_runs_empty_input_yields_empty() {
    let mut out = String::new();
    extract_printable_runs(&[], &mut out, 64);
    assert_eq!(out, "");
}

#[test]
fn append_utf16le_runs_budget_exhausted_exits_loop() {
    // 字节尚有剩余但 out 已达预算：while 第二条件 false 短路退出（BRDA 131 同型）。
    let mut out = String::from("xx");
    append_utf16le_runs(&utf16le("abcdefghij"), &mut out, 2);
    assert_eq!(out, "xx");
}

#[test]
fn append_ascii_runs_budget_exhausted_breaks() {
    let mut out = String::from("xx");
    append_ascii_runs(b"abcdefghij", &mut out, 2);
    assert_eq!(out, "xx");
}

#[test]
fn flush_run_respects_budget() {
    // run 达最小长度但 out 已满预算：不进 flush 分支。
    let mut out = String::from("xx");
    let mut run = String::from("aaaaaaaaaaaaaaaa");
    flush_run(&mut run, &mut out, 2);
    assert_eq!(out, "xx");
    assert!(run.is_empty());
}

#[test]
fn flush_run_appends_with_space_and_truncates() {
    let mut run = "aaaaaaaaaaaaaaaa".to_owned();
    let mut out = "existing".to_owned();
    flush_run(&mut run, &mut out, 100);
    assert_eq!(out, "existing aaaaaaaaaaaaaaaa");
    assert!(run.is_empty());

    let mut run2 = "bbbbbbbbcccccccc".to_owned();
    let mut out2 = String::new();
    flush_run(&mut run2, &mut out2, 10);
    assert!(out2.chars().count() <= 10, "got: {out2}");
    assert!(run2.is_empty());
}

#[test]
fn is_text_char_accepts_alnum_space_punct_rejects_control() {
    assert!(is_text_char('汉'));
    assert!(is_text_char('a'));
    assert!(is_text_char(' '));
    assert!(is_text_char('!'));
    assert!(!is_text_char('\u{0007}'));
    assert!(!is_text_char('\n'));
    assert!(!is_text_char('\u{0}'));
}

// ===== parse / extract_text 入口直调（release 实例覆盖 read-Err 防御分支） =====

fn build_cfb(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create cfb");
    for (name, data) in files {
        let path = format!("/{name}");
        let mut s = comp.create_stream(&path).expect("create stream");
        s.write_all(data).expect("write stream");
    }
    comp.into_inner().into_inner()
}

fn corrupt_start_sector(buf: &mut [u8], name: &str, sector: u32) {
    let name_utf16: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let pos = buf
        .windows(name_utf16.len())
        .position(|w| w == name_utf16.as_slice())
        .expect("dir entry name present");
    buf[pos + 116..pos + 120].copy_from_slice(&sector.to_le_bytes());
}

fn corrupt_stream_name(buf: &mut [u8], old_name: &str, new_name: &str) {
    let old_utf16: Vec<u8> = old_name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let pos = buf
        .windows(old_utf16.len())
        .position(|w| w == old_utf16.as_slice())
        .expect("dir entry name present");
    let new_utf16: Vec<u8> = new_name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert!(
        new_utf16.len() + 2 <= 64,
        "new name too long: {} bytes",
        new_utf16.len()
    );
    buf[pos..pos + new_utf16.len()].copy_from_slice(&new_utf16);
    for b in &mut buf[pos + new_utf16.len()..pos + 64] {
        *b = 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let name_len = (new_utf16.len() + 2) as u16;
    buf[pos + 64..pos + 66].copy_from_slice(&name_len.to_le_bytes());
}

#[test]
fn parse_invalid_cfb_returns_zeros() {
    let mut reader = Cursor::new(b"not a compound file".to_vec());
    assert_eq!(parse(&mut reader, "application/msword"), (0, 0));
}

#[test]
fn parse_cfb_without_summary_stream_returns_zeros() {
    let buf = build_cfb(&[("OtherStream", b"data")]);
    let mut reader = Cursor::new(buf);
    assert_eq!(parse(&mut reader, "application/msword"), (0, 0));
}

#[test]
fn parse_cfb_with_summary_stream_extracts_dates() {
    let propertyset = build_summary_propertyset(
        unix_to_filetime(1_700_000_000),
        unix_to_filetime(1_600_000_000),
    );
    let buf = build_cfb(&[("\u{5}SummaryInformation", &propertyset)]);
    let mut reader = Cursor::new(buf);
    assert_eq!(
        parse(&mut reader, "application/msword"),
        (1_700_000_000, 1_600_000_000)
    );
}

#[test]
fn parse_summary_read_error_returns_zeros() {
    let propertyset = build_summary_propertyset(
        unix_to_filetime(1_700_000_000),
        unix_to_filetime(1_600_000_000),
    );
    let mut buf = build_cfb(&[("\u{5}SummaryInformation", &propertyset)]);
    corrupt_start_sector(&mut buf, "\u{5}SummaryInformation", u32::MAX);
    let mut reader = Cursor::new(buf);
    assert_eq!(parse(&mut reader, "application/msword"), (0, 0));
}

#[test]
fn extract_text_invalid_cfb_returns_empty() {
    let mut reader = Cursor::new(b"garbage".to_vec());
    assert_eq!(extract_text(&mut reader, "application/msword", 256), "");
}

#[test]
fn extract_text_valid_cfb_extracts_ascii_runs() {
    let buf = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"this is a plain ascii sentence in the doc"),
    ]);
    let mut reader = Cursor::new(buf);
    let out = extract_text(&mut reader, "application/msword", 256);
    assert!(out.contains("this is a plain ascii sentence"), "got: {out}");
}

#[test]
fn extract_text_only_summary_stream_yields_empty() {
    let buf = build_cfb(&[("\u{5}SummaryInformation", &[0u8; 8])]);
    let mut reader = Cursor::new(buf);
    assert_eq!(extract_text(&mut reader, "application/msword", 256), "");
}

#[test]
fn extract_text_budget_zero_breaks_immediately() {
    let buf = build_cfb(&[("WordDocument", b"text")]);
    let mut reader = Cursor::new(buf);
    assert_eq!(extract_text(&mut reader, "application/msword", 0), "");
}

#[test]
fn extract_text_stream_read_error_skips() {
    let mut buf = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"this is a plain ascii sentence in the doc"),
    ]);
    corrupt_start_sector(&mut buf, "WordDocument", u32::MAX);
    let mut reader = Cursor::new(buf);
    assert_eq!(extract_text(&mut reader, "application/msword", 256), "");
}

#[test]
fn extract_text_open_stream_error_skips() {
    let mut buf = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"this is a plain ascii sentence in the doc"),
    ]);
    corrupt_stream_name(&mut buf, "WordDocument", "..");
    let mut reader = Cursor::new(buf);
    let out = extract_text(&mut reader, "application/msword", 256);
    assert_eq!(out, "", "got: {out}");
}
