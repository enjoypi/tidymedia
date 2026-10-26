#[test]
fn flush_run_truncates_at_boundary() {
    let mut run = "aaaaaaaabbbbbbbbcccccccc".to_owned();
    let mut out = String::new();
    flush_run(&mut run, &mut out, 6);
    assert!(out.chars().count() <= 6, "got: {out}");
}

use super::{extract_text, parse};

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
    assert!(new_utf16.len() + 2 <= 64, "new name too long");
    buf[pos..pos + new_utf16.len()].copy_from_slice(&new_utf16);
    for b in &mut buf[pos + new_utf16.len()..pos + 64] {
        *b = 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let name_len = (new_utf16.len() + 2) as u16;
    buf[pos + 64..pos + 66].copy_from_slice(&name_len.to_le_bytes());
}

/// 合法 CFB 容器但无 `\x05SummaryInformation` stream → `parse` 走 `open_stream` Err
/// 分支返 `(0, 0)`。cfb crate 是生产依赖（非 dev），可直接构造。
#[test]
fn parse_cfb_without_summary_stream_returns_zeros() {
    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create empty cfb");
    let mut s = comp
        .create_stream("/SomeOtherStream")
        .expect("create other stream");
    s.write_all(b"not summary").expect("write body");
    drop(s);
    let cursor = comp.into_inner();
    let buf = cursor.into_inner();
    let mut reader = Cursor::new(buf);
    let (created, modified) = parse(&mut reader, "application/msword");
    assert_eq!((created, modified), (0, 0));
}

#[test]
fn parse_invalid_cfb_returns_zeros() {
    let mut reader = Cursor::new(b"not a compound file".to_vec());
    assert_eq!(parse(&mut reader, "application/msword"), (0, 0));
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
fn extract_text_happy_path_and_error_arms() {
    let buf = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"this is a plain ascii sentence in the doc"),
    ]);
    let mut reader = Cursor::new(buf);
    assert!(
        extract_text(&mut reader, "application/msword", 256)
            .contains("this is a plain ascii sentence")
    );

    let mut bad = Cursor::new(b"garbage".to_vec());
    assert_eq!(extract_text(&mut bad, "application/msword", 256), "");

    let mut named = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"some text"),
    ]);
    corrupt_stream_name(&mut named, "WordDocument", "..");
    let mut reader = Cursor::new(named);
    assert_eq!(extract_text(&mut reader, "application/msword", 256), "");

    let mut read_err = build_cfb(&[
        ("\u{5}SummaryInformation", &[0u8; 8]),
        ("WordDocument", b"some text"),
    ]);
    corrupt_start_sector(&mut read_err, "WordDocument", u32::MAX);
    let mut reader = Cursor::new(read_err);
    assert_eq!(extract_text(&mut reader, "application/msword", 256), "");
}

/// 合法 CFB 含 `SummaryInformation` → `parse` 走 happy path 提取日期。
#[test]
fn parse_cfb_with_summary_stream_extracts_dates() {
    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create cfb");
    let propertyset = build_summary_propertyset(
        unix_to_filetime(1_700_000_000),
        unix_to_filetime(1_600_000_000),
    );
    let mut s = comp
        .create_stream("/\u{5}SummaryInformation")
        .expect("create summary stream");
    s.write_all(&propertyset).expect("write propertyset");
    drop(s);
    let cursor = comp.into_inner();
    let buf = cursor.into_inner();
    let mut reader = Cursor::new(buf);
    let (created, modified) = parse(&mut reader, "application/msword");
    assert_eq!(created, 1_700_000_000);
    assert_eq!(modified, 1_600_000_000);
}
